//! Linear chain synchronizer.
//!
//! Synchronizes the linear chain when node joins the network.
//!
//! Steps are:
//! 1. Fetch blocks up to initial, trusted hash (blocks are downloaded starting from trusted hash up
//! until Genesis).
//! 2. Fetch deploys of the lowest height block.
//! 3. Execute that block.
//! 4. Repeat steps 2-3 until trusted hash is reached.
//! 5. Transition to `SyncingDescendants` state.
//! 6. Fetch child block of highest block.
//! 7. Fetch deploys of that block.
//! 8. Execute that block.
//! 9. Repeat steps 6-8 as long as there's a child in the linear chain.
//!
//! The order of "download block – download deploys – execute" block steps differ,
//! in order to increase the chances of catching up with the linear chain quicker.
//! When synchronizing linear chain up to the trusted hash we cannot execute later blocks without
//! earlier ones. When we're syncing descendants, on the other hand, we can and we want to do it
//! ASAP so that we can start participating in consensus. That's why deploy fetching and block
//! execution is interleaved. If we had downloaded the whole chain, and then deploys, and then
//! execute (as we do in the first, SynchronizeTrustedHash, phase) it would have taken more time and
//! we might miss more eras.

mod blocks;
mod config;
mod deploys;
mod event;
mod metrics;
mod peers;
mod state;
mod traits;

use std::{cmp::Ordering, convert::Infallible, fmt::Display, mem};

use datasize::DataSize;
use prometheus::Registry;
use tracing::{error, info, trace, warn};

use self::event::BlockByHashResult;
use casper_types::{EraId, ProtocolVersion};

use super::{
    storage::{self, Storage},
    Component,
};
use crate::{
    components::contract_runtime::ExecutionPreState,
    effect::{EffectBuilder, EffectExt, Effects},
    fatal,
    types::{ActivationPoint, Block, BlockHash, BlockHeader, Chainspec, TimeDiff},
    NodeRng,
};
pub(crate) use config::Config;
pub(crate) use event::Event;
use event::{BlockByHeightResult, StopReason};
use metrics::Metrics;
pub(crate) use peers::PeersState;
pub(crate) use state::State;
pub(crate) use traits::ReactorEventT;

#[derive(DataSize, Debug)]
pub(crate) struct LinearChainSync<I> {
    peers: PeersState<I>,
    state: State,
    #[data_size(skip)]
    metrics: Metrics,
    /// The next upgrade activation point.
    /// When we download the switch block of an era immediately before the activation point,
    /// we need to shut down for an upgrade.
    next_upgrade_activation_point: Option<ActivationPoint>,
    stop_reason: Option<StopReason>,
    /// Key for storing the linear chain sync state.
    state_key: Vec<u8>,
    /// Shortest era that is allowed with the given protocol configuration.
    shortest_era: TimeDiff,
    /// Minimum round length that is allowed with the given protocol configuration.
    min_round_length: TimeDiff,
    /// Flag indicating whether we managed to sync at least one block.
    started_syncing: bool,
    /// The protocol version the node is currently running with.
    protocol_version: ProtocolVersion,
    initial_execution_pre_state: ExecutionPreState,
}

impl<I: Clone + PartialEq + 'static> LinearChainSync<I> {
    // TODO: fix this
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new<REv, Err>(
        registry: &Registry,
        effect_builder: EffectBuilder<REv>,
        chainspec: &Chainspec,
        storage: &Storage,
        trusted_hash: Option<BlockHash>,
        highest_block: Option<Block>,
        after_upgrade: bool,
        next_upgrade_activation_point: Option<ActivationPoint>,
        initial_execution_pre_state: ExecutionPreState,
        config: Config,
    ) -> Result<(Self, Effects<Event<I>>), Err>
    where
        REv: From<Event<I>> + Send,
        Err: From<prometheus::Error> + From<storage::Error>,
    {
        let timeout_event = effect_builder
            .set_timeout(config.get_sync_timeout().into())
            .event(|_| Event::InitializeTimeout);
        let protocol_version = chainspec.protocol_config.version;
        if let Some(state) = read_init_state(storage, chainspec)? {
            let linear_chain_sync = LinearChainSync::from_state(
                registry,
                chainspec,
                state,
                next_upgrade_activation_point,
                protocol_version,
                initial_execution_pre_state,
            )?;
            Ok((linear_chain_sync, timeout_event))
        } else {
            // Shortest era is the maximum of the two.
            let shortest_era: TimeDiff = std::cmp::max(
                chainspec.highway_config.min_round_length()
                    * chainspec.core_config.minimum_era_height,
                chainspec.core_config.era_duration,
            );

            let state = match trusted_hash {
                Some(hash) => {
                    // Check if we already have the trusted block in storage.
                    match storage.read_block(&hash) {
                        Ok(Some(block)) => {
                            if block.protocol_version() < protocol_version {
                                error!(trusted_hash=?hash,
                                    trusted_block_version=?block.protocol_version(),
                                    current_protocol_version=?protocol_version,
                                    "used trusted hash from before an upgrade. Use trusted hash from the new protocol version");
                            }
                            let switch_block_height = storage
                                .transactional_get_switch_block_by_era_id(
                                    block.header().era_id().value(),
                                )
                                .map(|b| b.height());
                            // If we already have the trusted block in storage, we can start syncing
                            // its descendants immediately.
                            State::sync_descendants(hash, block, switch_block_height)
                        }
                        Ok(None) => State::sync_trusted_hash(
                            hash,
                            highest_block.map(|block| block.take_header()),
                        ),
                        Err(error) => {
                            error!(?error, "error when reading block by hash from storage");
                            State::sync_trusted_hash(
                                hash,
                                highest_block.map(|block| block.take_header()),
                            )
                        }
                    }
                }
                None if after_upgrade => {
                    info!(
                        "No synchronization of the linear chain will be done because the node \
                        was started right after an upgrade without a trusted hash."
                    );
                    // Right after upgrade, no linear chain to synchronize.
                    State::Done(highest_block.map(Box::new))
                }
                None => {
                    if let Some(highest_block) = highest_block {
                        // No trusted hash, not immediately after upgrade.
                        // We will synchronize starting from the highest block we have.
                        // NOTE: This is unsafe and should only use the highest block if it can
                        // still be trusted – i.e. it's within the unbonding period.
                        State::sync_descendants(*highest_block.hash(), highest_block, None)
                    } else {
                        info!(
                            "No synchronization of the linear chain will be done because there \
                            is neither a trusted hash nor a highest block present."
                        );
                        State::Done(None)
                    }
                }
            };
            let state_key = create_state_key(chainspec);
            let linear_chain_sync = LinearChainSync {
                peers: PeersState::new(),
                state,
                metrics: Metrics::new(registry)?,
                next_upgrade_activation_point,
                stop_reason: None,
                state_key,
                shortest_era,
                min_round_length: chainspec.highway_config.min_round_length(),
                started_syncing: false,
                protocol_version,
                initial_execution_pre_state,
            };
            Ok((linear_chain_sync, timeout_event))
        }
    }

    /// Initialize `LinearChainSync` component from preloaded `State`.
    fn from_state(
        registry: &Registry,
        chainspec: &Chainspec,
        state: State,
        next_upgrade_activation_point: Option<ActivationPoint>,
        protocol_version: ProtocolVersion,
        initial_execution_pre_state: ExecutionPreState,
    ) -> Result<Self, prometheus::Error> {
        let state_key = create_state_key(chainspec);
        info!(?state, "reusing previous state");
        // Shortest era is the maximum of the two.
        let shortest_era: TimeDiff = std::cmp::max(
            chainspec.highway_config.min_round_length() * chainspec.core_config.minimum_era_height,
            chainspec.core_config.era_duration,
        );
        if matches!(state, State::None | State::Done(_)) {
            info!(
                "No synchronization of the linear chain will be done because the component is \
                already in State::Done or in State::None."
            );
        }
        Ok(LinearChainSync {
            peers: PeersState::new(),
            state,
            metrics: Metrics::new(registry)?,
            next_upgrade_activation_point,
            stop_reason: None,
            state_key,
            shortest_era,
            min_round_length: chainspec.highway_config.min_round_length(),
            started_syncing: false,
            protocol_version,
            initial_execution_pre_state,
        })
    }

    /// Add new block to linear chain.
    fn add_block(&mut self, block: Block) {
        self.started_syncing = true;
        match &mut self.state {
            State::None | State::Done(_) => {}
            State::SyncingTrustedHash { linear_chain, .. } => linear_chain.push(block),
            State::SyncingDescendants { latest_block, .. } => **latest_block = block,
        };
    }

    /// Returns `true` if we have finished syncing linear chain.
    pub(crate) fn is_synced(&self) -> bool {
        matches!(self.state, State::Done(_))
    }

    /// Returns `true` if we should stop for upgrade.
    pub(crate) fn stopped_for_upgrade(&self) -> bool {
        self.stop_reason == Some(StopReason::ForUpgrade)
    }

    /// Returns `true` if we should stop for downgrade.
    pub(crate) fn stopped_for_downgrade(&self) -> bool {
        self.stop_reason == Some(StopReason::ForDowngrade)
    }

    fn block_downloaded<REv>(
        &mut self,
        rng: &mut NodeRng,
        effect_builder: EffectBuilder<REv>,
        block: &Block,
    ) -> Effects<Event<I>>
    where
        I: Send + 'static,
        REv: ReactorEventT<I>,
    {
        self.peers.reset(rng);
        self.state.block_downloaded(block);
        self.add_block(block.clone());
        match &self.state {
            State::None | State::Done(_) => {
                error!(state=?self.state, "block downloaded when in incorrect state.");
                fatal!(effect_builder, "block downloaded in incorrect state").ignore()
            }
            State::SyncingTrustedHash {
                highest_block_header,
                ..
            } => {
                let should_start_downloading_deploys = highest_block_header
                    .as_ref()
                    .map(|hdr| hdr.hash() == *block.header().parent_hash())
                    .unwrap_or(false)
                    || block.header().is_genesis_child();
                if should_start_downloading_deploys {
                    info!("linear chain downloaded. Start downloading deploys.");
                    effect_builder
                        .immediately()
                        .event(move |_| Event::StartDownloadingDeploys)
                } else {
                    self.fetch_next_block(effect_builder, rng, block)
                }
            }
            State::SyncingDescendants { .. } => {
                // When synchronizing descendants, we want to download block and execute it
                // before trying to download the next block in linear chain.
                self.fetch_next_block_deploys(effect_builder)
            }
        }
    }

    fn mark_done(&mut self, latest_block: Option<Block>) {
        let latest_block = latest_block.map(Box::new);
        self.state = State::Done(latest_block);
    }

    /// Handles an event indicating that a linear chain block has been executed and handled by
    /// consensus component. This is a signal that we can safely continue with the next blocks,
    /// without worrying about timing and/or ordering issues.
    /// Returns effects that are created as a response to that event.
    fn block_handled<REv>(
        &mut self,
        rng: &mut NodeRng,
        effect_builder: EffectBuilder<REv>,
        block: &Block,
    ) -> Effects<Event<I>>
    where
        I: Send + 'static,
        REv: ReactorEventT<I>,
    {
        let height = block.height();
        let hash = block.hash();
        trace!(%hash, %height, "downloaded linear chain block.");
        if block.header().is_switch_block() {
            self.state.set_last_switch_block_height(block.height());
        }
        if block.header().is_switch_block() && self.should_upgrade(block.header().era_id()) {
            info!(
                era = block.header().era_id().value(),
                "shutting down for upgrade"
            );
            return effect_builder
                .immediately()
                .event(|_| Event::InitUpgradeShutdown);
        }
        // Reset peers before creating new requests.
        self.peers.reset(rng);
        let block_height = block.height();
        let curr_state = mem::replace(&mut self.state, State::None);
        match curr_state {
            State::None | State::Done(_) => {
                error!(state=?self.state, "block handled when in incorrect state.");
                fatal!(effect_builder, "block handled in incorrect state").ignore()
            }
            // Keep syncing from genesis if we haven't reached the trusted block hash
            State::SyncingTrustedHash {
                highest_block_seen,
                ref latest_block,
                ..
            } if highest_block_seen != block_height => {
                match latest_block.as_ref() {
                    Some(expected) if expected != block => {
                        error!(
                            ?expected, got=?block,
                            "block execution result doesn't match received block"
                        );
                        return fatal!(effect_builder, "unexpected block execution result")
                            .ignore();
                    }
                    None => {
                        error!("block execution results received when not expected");
                        return fatal!(effect_builder, "unexpected block execution results.")
                            .ignore();
                    }
                    Some(_) => (),
                }
                self.state = curr_state;
                self.fetch_next_block_deploys(effect_builder)
            }
            // Otherwise transition to State::SyncingDescendants
            State::SyncingTrustedHash {
                highest_block_seen,
                trusted_hash,
                ref latest_block,
                last_switch_block_height,
                ..
            } => {
                if highest_block_seen != block_height {
                    error!(?highest_block_seen, downloaded_block_height=?block_height, "downloaded unexpected block");
                    return fatal!(
                        effect_builder,
                        "downloaded unexpected block when syncing trusted hash"
                    )
                    .ignore();
                }
                match latest_block.as_ref() {
                    Some(expected) if expected != block => {
                        error!(
                            ?expected, got=?block,
                            "block execution result doesn't match received block"
                        );
                        return fatal!(effect_builder, "unexpected block execution result")
                            .ignore();
                    }
                    None => {
                        error!("block execution results received when not expected");
                        return fatal!(effect_builder, "unexpected block execution results.")
                            .ignore();
                    }
                    Some(_) => (),
                }
                info!(%block_height, "Finished synchronizing linear chain up until trusted hash.");
                let peer = self.peers.random_unsafe();
                // Kick off syncing trusted hash descendants.
                self.state =
                    State::sync_descendants(trusted_hash, block.clone(), last_switch_block_height);
                blocks::fetch_block_at_height(effect_builder, peer, block_height + 1)
            }
            State::SyncingDescendants {
                ref latest_block,
                last_switch_block_height,
                ..
            } => {
                if latest_block.as_ref() != block {
                    error!(
                        expected=?*latest_block, got=?block,
                        "block execution result doesn't match received block"
                    );
                    return fatal!(effect_builder, "unexpected block execution result").ignore();
                }
                if self.is_currently_active_era(latest_block, last_switch_block_height) {
                    info!(
                        hash=?block.hash(),
                        height=?block.header().height(),
                        era=block.header().era_id().value(),
                        "downloaded a block in the current era. finished synchronization"
                    );
                    self.mark_done(Some(*latest_block.clone()));
                    return Effects::new();
                }
                self.state = curr_state;
                self.fetch_next_block(effect_builder, rng, block)
            }
        }
    }

    // Returns whether we've just downloaded a block in a currently active era.
    fn is_currently_active_era(
        &self,
        block: &Block,
        last_switch_block_height: Option<u64>,
    ) -> bool {
        let last_switch_block_height = last_switch_block_height.unwrap_or(0);
        // This is the number of blocks that have we already know of from the era the current block
        // is in.
        let past_blocks_in_this_era = block.height().saturating_sub(last_switch_block_height);
        // `self.shortest_era - self.min_round_length * past_blocks_in_this_era` is the estimated
        // time left to the end of the era the current block is in; if less time than that has
        // passed since the current block, this is most likely still the current era and we can
        // return `true`.
        // We add `min_round_length * past_blocks_in_this_era` to the left side instead of
        // subtracting it from the right side to avoid underflows (TimeDiffs can't represent values
        // less than 0).
        block.header().timestamp().elapsed() + self.min_round_length * past_blocks_in_this_era
            < self.shortest_era
    }

    /// Returns effects for fetching next block's deploys.
    fn fetch_next_block_deploys<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event<I>>
    where
        I: Send + 'static,
        REv: ReactorEventT<I>,
    {
        let peer = self.peers.random_unsafe();

        let next_block = match &mut self.state {
            State::None | State::Done(_) => {
                error!(state=?self.state, "tried fetching next block when in wrong state");
                return fatal!(
                    effect_builder,
                    "tried fetching next block when in wrong state"
                )
                .ignore();
            }
            State::SyncingTrustedHash {
                linear_chain,
                latest_block,
                ..
            } => match linear_chain.pop() {
                None => None,
                Some(block) => {
                    // Update `latest_block` so that we can verify whether result of execution
                    // matches the expected value.
                    latest_block.replace(block.clone());
                    Some(block)
                }
            },
            State::SyncingDescendants { latest_block, .. } => Some((**latest_block).clone()),
        };

        next_block.map_or_else(
            || {
                warn!("tried fetching next block deploys when there was no block.");
                Effects::new()
            },
            |block| {
                self.metrics.reset_start_time();
                deploys::fetch_block_deploys(effect_builder, peer, block)
            },
        )
    }

    fn fetch_next_block<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        block: &Block,
    ) -> Effects<Event<I>>
    where
        I: Send + 'static,
        REv: ReactorEventT<I>,
    {
        self.peers.reset(rng);
        let peer = self.peers.random_unsafe();
        match self.state {
            State::SyncingTrustedHash { .. } => {
                let parent_hash = *block.header().parent_hash();
                self.metrics.reset_start_time();
                blocks::fetch_block_by_hash(effect_builder, peer, parent_hash)
            }
            State::SyncingDescendants { .. } => {
                let next_height = block.height() + 1;
                self.metrics.reset_start_time();
                blocks::fetch_block_at_height(effect_builder, peer, next_height)
            }
            State::Done(_) | State::None => {
                error!(state=?self.state, "tried fetching next block when in wrong state");
                fatal!(
                    effect_builder,
                    "tried fetching next block when in wrong state"
                )
                .ignore()
            }
        }
    }

    fn handle_upgrade_shutdown<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event<I>>
    where
        I: Send + 'static,
        REv: ReactorEventT<I>,
    {
        if self.state.is_done() || self.state.is_none() {
            error!(state=?self.state, "shutdown for upgrade initiated when in wrong state");
            return fatal!(
                effect_builder,
                "shutdown for upgrade initiated when in wrong state"
            )
            .ignore();
        }
        effect_builder
            .save_state(self.state_key.clone().into(), Some(self.state.clone()))
            .event(|_| Event::Shutdown(StopReason::ForUpgrade))
    }

    pub(crate) fn latest_block(&self) -> Option<&Block> {
        match &self.state {
            State::SyncingTrustedHash { latest_block, .. } => Option::as_ref(latest_block),
            State::SyncingDescendants { latest_block, .. } => Some(latest_block),
            State::Done(latest_block) => latest_block.as_deref(),
            State::None => None,
        }
    }

    pub(crate) fn into_maybe_latest_block_header(self) -> Option<BlockHeader> {
        match self.state {
            State::SyncingTrustedHash { latest_block, .. } => latest_block.map(Block::take_header),
            State::SyncingDescendants { latest_block, .. } => Some(latest_block.take_header()),
            State::Done(maybe_latest_block) => {
                maybe_latest_block.map(|latest_block| latest_block.take_header())
            }
            State::None => None,
        }
    }

    fn should_upgrade(&self, era_id: EraId) -> bool {
        should_upgrade(self.next_upgrade_activation_point, era_id)
    }

    fn set_last_block_if_syncing_trusted_hash(&mut self, block: &Block) {
        if let State::SyncingTrustedHash {
            ref mut latest_block,
            ..
        } = &mut self.state
        {
            *latest_block = Box::new(Some(block.clone()));
        }
        self.state.block_downloaded(block);
    }
}

impl<I, REv> Component<REv> for LinearChainSync<I>
where
    I: Display + Clone + Send + PartialEq + 'static,
    REv: ReactorEventT<I>,
{
    type Event = Event<I>;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Start(init_peer) => {
                match &self.state {
                    State::None => {
                        // No syncing configured.
                        trace!("received `Start` event when in {} state.", self.state);
                        Effects::new()
                    }
                    State::Done(_) => {
                        // Illegal states for syncing start.
                        error!("should not have received `Start` event when in `Done` state.",);
                        Effects::new()
                    }
                    State::SyncingDescendants { latest_block, .. } => {
                        let next_block_height = latest_block.height() + 1;
                        info!(?next_block_height, "start synchronization");
                        self.metrics.reset_start_time();
                        blocks::fetch_block_at_height(effect_builder, init_peer, next_block_height)
                    }
                    State::SyncingTrustedHash { trusted_hash, .. } => {
                        trace!(?trusted_hash, "start synchronization");
                        // Start synchronization.
                        self.metrics.reset_start_time();
                        blocks::fetch_block_by_hash(effect_builder, init_peer, *trusted_hash)
                    }
                }
            }
            Event::GetBlockHeightResult(block_height, fetch_result) => {
                match fetch_result {
                    BlockByHeightResult::Absent(peer) => {
                        self.metrics.observe_get_block_by_height();
                        trace!(
                            %block_height, %peer,
                            "failed to download block by height. Trying next peer"
                        );
                        self.peers.failure(&peer);
                        match self.peers.random() {
                            None => {
                                // `block_height` not found on any of the peers.
                                // We have synchronized all, currently existing, descendants of
                                // trusted hash.
                                info!(
                                    "finished synchronizing descendants of the trusted hash. \
                                     cleaning state."
                                );
                                self.mark_done(self.latest_block().cloned());
                                Effects::new()
                            }
                            Some(peer) => {
                                self.metrics.reset_start_time();
                                blocks::fetch_block_at_height(effect_builder, peer, block_height)
                            }
                        }
                    }
                    BlockByHeightResult::FromStorage(block) => {
                        // We shouldn't get invalid data from the storage.
                        // If we do, it's a bug.
                        if block.height() != block_height {
                            return fatal!(effect_builder, "block height mismatch").ignore();
                        }
                        trace!(%block_height, "Linear block found in the local storage.");
                        // We need to update internal state of the `LinearChainSync` component
                        // but we ignore the usual effects. If that block is already in storage,
                        // it means we must have "acted" on it and processed these effects in the
                        // past. Now, we can continue downloading the next
                        // block in the chain - one with height + 1.
                        let _ = self.block_downloaded(rng, effect_builder, &block);
                        let _ = self.block_handled(rng, effect_builder, &block);
                        self.fetch_next_block(effect_builder, rng, &block)
                    }
                    BlockByHeightResult::FromPeer(block, peer) => {
                        self.metrics.observe_get_block_by_height();
                        trace!(%block_height, %peer, "linear chain block downloaded from a peer");
                        if block.height() != block_height
                            || *block.header().parent_hash() != *self.latest_block().unwrap().hash()
                        {
                            warn!(
                                %peer,
                                got_height = block.height(),
                                expected_height = block_height,
                                got_parent = %block.header().parent_hash(),
                                expected_parent = %self.latest_block().unwrap().hash(),
                                "block mismatch",
                            );
                            // NOTE: Signal misbehaving validator to networking layer.
                            self.peers.ban(&peer);
                            return self.handle_event(
                                effect_builder,
                                rng,
                                Event::GetBlockHeightResult(
                                    block_height,
                                    BlockByHeightResult::Absent(peer),
                                ),
                            );
                        }
                        if block.protocol_version() != self.protocol_version {
                            warn!(
                                %peer,
                                protocol_version = %self.protocol_version,
                                block_version = %block.protocol_version(),
                                "block protocol version mismatch",
                            );
                            // NOTE: Signal misbehaving validator to networking layer.
                            self.peers.ban(&peer);
                            return self.handle_event(
                                effect_builder,
                                rng,
                                Event::GetBlockHeightResult(
                                    block_height,
                                    BlockByHeightResult::Absent(peer),
                                ),
                            );
                        }
                        self.peers.success(peer);
                        self.block_downloaded(rng, effect_builder, &block)
                    }
                }
            }
            Event::GetBlockHashResult(block_hash, fetch_result) => {
                match fetch_result {
                    BlockByHashResult::Absent(peer) => {
                        self.metrics.observe_get_block_by_hash();
                        trace!(
                            %block_hash, %peer,
                            "failed to download block by hash. Trying next peer"
                        );
                        self.peers.failure(&peer);
                        match self.peers.random() {
                            None if self.started_syncing => {
                                error!(
                                    %block_hash,
                                    "could not download linear block from any of the peers."
                                );
                                fatal!(effect_builder, "failed to synchronize linear chain")
                                    .ignore()
                            }
                            None => {
                                warn!(
                                    "run out of peers before managed to start syncing. \
                                    Resetting peers' list and continuing"
                                );
                                self.peers.reset(rng);
                                self.metrics.reset_start_time();
                                blocks::fetch_block_by_hash(effect_builder, peer, block_hash)
                            }
                            Some(peer) => {
                                self.metrics.reset_start_time();
                                blocks::fetch_block_by_hash(effect_builder, peer, block_hash)
                            }
                        }
                    }
                    BlockByHashResult::FromStorage(block) => {
                        // We shouldn't get invalid data from the storage.
                        // If we do, it's a bug.
                        if *block.hash() != block_hash {
                            return fatal!(effect_builder, "block hash mismatch").ignore();
                        }
                        trace!(%block_hash, "Linear block found in the local storage.");
                        // We hit a block that we already had in the storage - which should mean
                        // that we also have all of its ancestors, so we switch to traversing the
                        // chain forwards and downloading the deploys.
                        // We don't want to download and execute a block we already have, so
                        // instead of calling self.block_downloaded(), we take a shortcut:
                        self.set_last_block_if_syncing_trusted_hash(&block);
                        self.block_handled(rng, effect_builder, &block)
                    }
                    BlockByHashResult::FromPeer(block, peer) => {
                        self.metrics.observe_get_block_by_hash();
                        trace!(%block_hash, %peer, "linear chain block downloaded from a peer");
                        let header_hash = block.header().hash();
                        if header_hash != block_hash || header_hash != *block.hash() {
                            warn!(
                                "Block hash mismatch. Expected {} got {} from {}.\
                                 Block claims to have hash {}. Disconnecting.",
                                block_hash,
                                header_hash,
                                block.hash(),
                                peer
                            );
                            // NOTE: Signal misbehaving validator to networking layer.
                            self.peers.ban(&peer);
                            return self.handle_event(
                                effect_builder,
                                rng,
                                Event::GetBlockHashResult(
                                    block_hash,
                                    BlockByHashResult::Absent(peer),
                                ),
                            );
                        }
                        self.peers.success(peer);
                        self.block_downloaded(rng, effect_builder, &block)
                    }
                }
            }
            Event::GetDeploysResult(fetch_result) => {
                self.metrics.observe_get_deploys();
                match fetch_result {
                    event::DeploysResult::Found(block_to_execute) => {
                        // Before we execute, check if the protocol version is correct - if not,
                        // stop for an upgrade/downgrade
                        match self
                            .protocol_version
                            .cmp(&block_to_execute.protocol_version())
                        {
                            Ordering::Less => {
                                warn!(
                                    block_hash = %block_to_execute.hash(),
                                    our_version = %self.protocol_version,
                                    block_version = %block_to_execute.protocol_version(),
                                    "attempting to execute a block with a newer protocol version;\
                                    shutting down to upgrade"
                                );
                                return effect_builder
                                    .immediately()
                                    .event(|_| Event::Shutdown(StopReason::ForUpgrade));
                            }
                            Ordering::Greater => {
                                warn!(
                                    block_hash = %block_to_execute.hash(),
                                    our_version = %self.protocol_version,
                                    block_version = %block_to_execute.protocol_version(),
                                    "attempting to execute a block with an older protocol version;\
                                    shutting down to downgrade"
                                );
                                return effect_builder
                                    .immediately()
                                    .event(|_| Event::Shutdown(StopReason::ForDowngrade));
                            }
                            _ => (),
                        }
                        let block_hash = block_to_execute.hash();
                        trace!(%block_hash, "deploys for linear chain block found");
                        // Reset used peers so we can download next block with the full set.
                        self.peers.reset(rng);
                        // Execute block
                        blocks::execute_block(
                            effect_builder,
                            *block_to_execute,
                            self.initial_execution_pre_state.clone(),
                        )
                        .ignore()
                    }
                    event::DeploysResult::NotFound(block, peer) => {
                        let block_hash = block.hash();
                        trace!(
                            %block_hash, %peer,
                            "deploy for linear chain block not found. Trying next peer"
                        );
                        self.peers.failure(&peer);
                        match self.peers.random() {
                            None => {
                                error!(
                                    %block_hash,
                                    "could not download deploys from linear chain block."
                                );
                                fatal!(effect_builder, "failed to download linear chain deploys")
                                    .ignore()
                            }
                            Some(peer) => {
                                self.metrics.reset_start_time();
                                deploys::fetch_block_deploys(effect_builder, peer, *block)
                            }
                        }
                    }
                }
            }
            Event::StartDownloadingDeploys => {
                // Start downloading deploys from the first block of the linear chain.
                self.peers.reset(rng);
                self.fetch_next_block_deploys(effect_builder)
            }
            Event::NewPeerConnected(peer_id) => {
                trace!(%peer_id, "new peer connected");
                // Add to the set of peers we can request things from.
                let mut effects = Effects::new();
                if self.peers.is_empty() {
                    // First peer connected, start downloading.
                    let cloned_peer_id = peer_id.clone();
                    effects.extend(
                        effect_builder
                            .immediately()
                            .event(move |_| Event::Start(cloned_peer_id)),
                    );
                }
                self.peers.push(peer_id);
                effects
            }
            Event::BlockHandled(block) => {
                let block_height = block.height();
                let block_hash = *block.hash();
                let effects = self.block_handled(rng, effect_builder, &block);
                trace!(%block_height, %block_hash, "block handled");
                effects
            }
            Event::GotUpgradeActivationPoint(next_upgrade_activation_point) => {
                trace!(%next_upgrade_activation_point, "new activation point");
                self.next_upgrade_activation_point = Some(next_upgrade_activation_point);
                Effects::new()
            }
            Event::InitUpgradeShutdown => {
                info!("shutdown initiated");
                // Serialize and store state.
                self.handle_upgrade_shutdown(effect_builder)
            }
            Event::Shutdown(reason) => {
                info!(?reason, "ready for shutdown");
                self.stop_reason = Some(reason);
                Effects::new()
            }
            Event::InitializeTimeout => {
                if !self.started_syncing {
                    info!("hasn't downloaded any blocks in expected time window. Shutting down…");
                    fatal!(effect_builder, "no syncing progress, shutting down…").ignore()
                } else {
                    Effects::new()
                }
            }
        }
    }
}

fn should_upgrade(next_upgrade: Option<ActivationPoint>, block_era_id: EraId) -> bool {
    match next_upgrade {
        None => false,
        Some(activation_point) => activation_point.should_upgrade(&block_era_id),
    }
}

/// Returns key in the database, under which the LinearChainSync's state is stored.
fn create_state_key(chainspec: &Chainspec) -> Vec<u8> {
    format!(
        "linear_chain_sync:network_name={}",
        chainspec.network_config.name.clone()
    )
    .into()
}

/// Deserialized vector of bytes into `LinearChainSync::State`.
/// Panics on deserialization errors.
fn deserialize_state(serialized_state: &[u8]) -> Option<State> {
    bincode::deserialize(serialized_state).unwrap_or_else(|error| {
        // Panicking here should not corrupt the state of any component as it's done in the
        // constructor.
        panic!(
            "could not deserialize state from storage, error {:?}",
            error
        )
    })
}

/// Reads the `LinearChainSync's` state from storage, if any.
/// Panics on deserialization errors.
fn read_init_state(
    storage: &Storage,
    chainspec: &Chainspec,
) -> Result<Option<State>, storage::Error> {
    let key = create_state_key(chainspec);
    if let Some(bytes) = storage.read_state_store(&key)? {
        Ok(deserialize_state(&bytes))
    } else {
        Ok(None)
    }
}

/// Cleans the linear chain state storage.
/// May fail with storage error.
pub(crate) fn clean_linear_chain_state(
    storage: &Storage,
    chainspec: &Chainspec,
) -> Result<bool, storage::Error> {
    let key = create_state_key(chainspec);
    storage.del_state_store(key)
}
