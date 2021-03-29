//! Linear chain synchronizer.

mod error;
mod event;
mod metrics;
mod operations;
mod peers;
mod state;
mod traits;

use std::{convert::Infallible, fmt::Display, mem, str::FromStr};

use datasize::DataSize;
use prometheus::Registry;
use tracing::{error, info, trace, warn};

use casper_execution_engine::{shared::stored_value::StoredValue, storage::trie::Trie};
use casper_types::{EraId, Key};

use super::{
    fetcher::FetchResult,
    storage::{self, Storage},
    Component,
};
use crate::{
    effect::{
        requests::{ContractRuntimeRequest, FetcherRequest, NetworkInfoRequest},
        EffectBuilder, EffectExt, EffectOptionExt, Effects,
    },
    fatal,
    types::{
        ActivationPoint, Block, BlockByHeight, BlockHash, BlockHeader, Chainspec, FinalizedBlock,
        TimeDiff,
    },
    NodeRng,
};

use crate::effect::{announcements::ControlAnnouncement, requests::StateStoreRequest};
pub use event::Event;
use event::{BlockByHashResult, BlockByHeightResult, DeploysResult};
pub use metrics::LinearChainSyncMetrics;
pub use peers::PeersState;
pub use state::State;
use std::fmt::Debug;
pub use traits::ReactorEventT;

#[derive(DataSize, Debug)]
pub(crate) struct LinearChainSync<I> {
    init_hash: Option<BlockHash>,
    chainspec: Chainspec,
    started: bool,
    #[data_size(skip)]
    metrics: LinearChainSyncMetrics,

    peers: PeersState<I>,
    state: State,
    /// The next upgrade activation point.
    /// When we download the switch block of an era immediately before the activation point,
    /// we need to shut down for an upgrade.
    next_upgrade_activation_point: Option<ActivationPoint>,
    stop_for_upgrade: bool,
    /// Key for storing the linear chain sync state.
    state_key: Vec<u8>,
    /// Acceptable drift between the block creation and now.
    /// If less than than this has passed we will consider syncing as finished.
    acceptable_drift: TimeDiff,
    /// Shortest era that is allowed with the given protocol configuration.
    shortest_era: TimeDiff,
    /// Flag indicating whether we managed to sync at least one block.
    started_syncing: bool,
}

impl<I: Clone + PartialEq + 'static> LinearChainSync<I> {
    // TODO: fix this
    #[allow(clippy::too_many_arguments)]
    pub fn new<REv, Err>(
        registry: &Registry,
        effect_builder: EffectBuilder<REv>,
        chainspec: &Chainspec,
        storage: &Storage,
        init_hash: Option<BlockHash>,
        highest_block: Option<Block>,
        next_upgrade_activation_point: Option<ActivationPoint>,
    ) -> Result<(Self, Effects<Event<I>>), Err>
    where
        REv: From<Event<I>> + Send,
        Err: From<prometheus::Error> + From<storage::Error>,
    {
        // set timeout to 5 minutes after now.
        let five_minutes = TimeDiff::from_str("5minutes").unwrap();
        let timeout_event = effect_builder
            .set_timeout(five_minutes.into())
            .event(|_| Event::InitializeTimeout);
        if let Some(state) = read_init_state(storage, chainspec)? {
            let linear_chain_sync = LinearChainSync::from_state(
                init_hash,
                registry,
                chainspec,
                state,
                next_upgrade_activation_point,
            )?;
            Ok((linear_chain_sync, timeout_event))
        } else {
            let acceptable_drift = chainspec.highway_config.max_round_length();
            // Shortest era is the maximum of the two.
            let shortest_era: TimeDiff = std::cmp::max(
                chainspec.highway_config.min_round_length()
                    * chainspec.core_config.minimum_era_height,
                chainspec.core_config.era_duration,
            );
            let state = init_hash.map_or(State::None, |init_hash| {
                State::sync_trusted_hash(
                    init_hash,
                    highest_block.map(|block| block.header().clone()),
                )
            });

            let state_key = create_state_key(&chainspec);
            let linear_chain_sync = LinearChainSync {
                init_hash,
                started: false,
                chainspec: chainspec.clone(),
                peers: PeersState::new(),
                state,
                metrics: LinearChainSyncMetrics::new(registry)?,
                next_upgrade_activation_point,
                stop_for_upgrade: false,
                state_key,
                acceptable_drift,
                shortest_era,
                started_syncing: false,
            };
            Ok((linear_chain_sync, timeout_event))
        }
    }

    /// Initialize `LinearChainSync` component from preloaded `State`.
    fn from_state(
        init_hash: Option<BlockHash>,
        registry: &Registry,
        chainspec: &Chainspec,
        state: State,
        next_upgrade_activation_point: Option<ActivationPoint>,
    ) -> Result<Self, prometheus::Error> {
        let state_key = create_state_key(chainspec);
        info!(?state, "reusing previous state");
        let acceptable_drift = chainspec.highway_config.max_round_length();
        // Shortest era is the maximum of the two.
        let shortest_era: TimeDiff = std::cmp::max(
            chainspec.highway_config.min_round_length() * chainspec.core_config.minimum_era_height,
            chainspec.core_config.era_duration,
        );
        Ok(LinearChainSync {
            init_hash,
            started: false,
            chainspec: chainspec.clone(),
            peers: PeersState::new(),
            state,
            metrics: LinearChainSyncMetrics::new(registry)?,
            next_upgrade_activation_point,
            stop_for_upgrade: false,
            state_key,
            acceptable_drift,
            shortest_era,
            started_syncing: false,
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
    pub fn is_synced(&self) -> bool {
        matches!(self.state, State::None | State::Done(_))
    }

    /// Returns `true` if we should stop for upgrade.
    pub fn stopped_for_upgrade(&self) -> bool {
        self.stop_for_upgrade
    }

    fn block_downloaded<REv>(
        &mut self,
        rng: &mut NodeRng,
        effect_builder: EffectBuilder<REv>,
        block: &Block,
    ) -> Effects<Event<I>>
    where
        I: Send + 'static,
        REv: ReactorEventT<I> + From<ControlAnnouncement>,
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

    fn mark_done(&mut self) {
        let latest_block = self.latest_block().cloned().map(Box::new);
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
        block: Block,
    ) -> Effects<Event<I>>
    where
        I: Send + 'static,
        REv: ReactorEventT<I> + From<ControlAnnouncement>,
    {
        let height = block.height();
        let hash = block.hash();
        trace!(%hash, %height, "downloaded linear chain block.");
        if block.header().is_switch_block() {
            self.state.new_switch_block(&block);
        }
        if block.header().is_switch_block() && self.should_upgrade(block.header().era_id()) {
            info!(
                era = <u64>::from(block.header().era_id()),
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
                    Some(expected) if expected != &block => {
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
                maybe_switch_block,
                ..
            } => {
                assert_eq!(highest_block_seen, block_height);
                match latest_block.as_ref() {
                    Some(expected) if expected != &block => {
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
                self.state = State::sync_descendants(trusted_hash, block, maybe_switch_block);
                fetch_block_at_height(effect_builder, peer, block_height + 1)
            }
            State::SyncingDescendants {
                ref latest_block,
                ref maybe_switch_block,
                ..
            } => {
                if latest_block.as_ref() != &block {
                    error!(
                        expected=?*latest_block, got=?block,
                        "block execution result doesn't match received block"
                    );
                    return fatal!(effect_builder, "unexpected block execution result").ignore();
                }
                if self.is_recent_block(&block) {
                    info!(
                        hash=?block.hash(),
                        height=?block.header().height(),
                        era=<u64>::from(block.header().era_id()),
                        "downloaded recent block. finished synchronization"
                    );
                    self.mark_done();
                    return Effects::new();
                }
                if self.is_currently_active_era(&maybe_switch_block) {
                    info!(
                        hash=?block.hash(),
                        height=?block.header().height(),
                        era=<u64>::from(block.header().era_id()),
                        "downloaded switch block of a new era. finished synchronization"
                    );
                    self.mark_done();
                    return Effects::new();
                }
                self.state = curr_state;
                self.fetch_next_block(effect_builder, rng, &block)
            }
        }
    }

    // Returns whether `block` can be considered the tip of the chain.
    fn is_recent_block(&self, block: &Block) -> bool {
        // Check if block was created "recently".
        block.header().timestamp().elapsed() <= self.acceptable_drift
    }

    // Returns whether we've just downloaded a switch block of a currently active era.
    fn is_currently_active_era(&self, maybe_switch_block: &Option<Box<Block>>) -> bool {
        match maybe_switch_block {
            Some(switch_block) => switch_block.header().timestamp().elapsed() < self.shortest_era,
            None => false,
        }
    }

    /// Returns effects for fetching next block's deploys.
    fn fetch_next_block_deploys<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event<I>>
    where
        I: Send + 'static,
        REv: ReactorEventT<I> + From<ControlAnnouncement>,
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
                fetch_block_deploys(effect_builder, peer, block)
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
        REv: ReactorEventT<I> + From<ControlAnnouncement>,
    {
        self.peers.reset(rng);
        let peer = self.peers.random_unsafe();
        match self.state {
            State::SyncingTrustedHash { .. } => {
                let parent_hash = *block.header().parent_hash();
                self.metrics.reset_start_time();
                fetch_block_by_hash(effect_builder, peer, parent_hash)
            }
            State::SyncingDescendants { .. } => {
                let next_height = block.height() + 1;
                self.metrics.reset_start_time();
                fetch_block_at_height(effect_builder, peer, next_height)
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
        REv: ReactorEventT<I> + From<ControlAnnouncement> + From<StateStoreRequest>,
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
            .event(|_| Event::Shutdown(true))
    }

    pub(crate) fn latest_block(&self) -> Option<&Block> {
        match &self.state {
            State::SyncingTrustedHash { latest_block, .. } => Option::as_ref(&*latest_block),
            State::SyncingDescendants { latest_block, .. } => Some(&*latest_block),
            State::Done(latest_block) => latest_block.as_deref(),
            State::None => None,
        }
    }

    fn should_upgrade(&self, era_id: EraId) -> bool {
        match self.next_upgrade_activation_point {
            None => false,
            Some(activation_point) => activation_point.should_upgrade(&era_id),
        }
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
    I: Display + Debug + Clone + Send + Sync + PartialEq + 'static + Eq + std::hash::Hash,
    REv: ReactorEventT<I>
        + From<ContractRuntimeRequest>
        + From<FetcherRequest<I, BlockHeader>>
        + From<FetcherRequest<I, Trie<Key, StoredValue>>>
        + From<NetworkInfoRequest<I>>
        + From<ControlAnnouncement>,
{
    type Event = Event<I>;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        let init_hash = match self.init_hash {
            Some(trusted_hash) => trusted_hash,
            // If there is no trusted hash, there is nothing to sync and we don't emit any effects.
            None => {
                warn!(%event, "No trusted block hash, joiner dropping event");
                return Effects::new();
            }
        };

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
                        fetch_block_at_height(effect_builder, init_peer, next_block_height)
                    }
                    State::SyncingTrustedHash { trusted_hash, .. } => {
                        trace!(?trusted_hash, "start synchronization");
                        // Start synchronization.
                        self.metrics.reset_start_time();
                        fetch_block_by_hash(effect_builder, init_peer, *trusted_hash)
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
                                self.mark_done();
                                Effects::new()
                            }
                            Some(peer) => {
                                self.metrics.reset_start_time();
                                fetch_block_at_height(effect_builder, peer, block_height)
                            }
                        }
                    }
                    BlockByHeightResult::FromStorage(block) => {
                        // We shouldn't get invalid data from the storage.
                        // If we do, it's a bug.
                        assert_eq!(block.height(), block_height, "Block height mismatch.");
                        trace!(%block_height, "Linear block found in the local storage.");
                        // When syncing descendants of a trusted hash, we might have some of
                        // them in our local storage. If that's the case, just continue.
                        self.block_downloaded(rng, effect_builder, &block)
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
                                fetch_block_by_hash(effect_builder, peer, block_hash)
                            }
                            Some(peer) => {
                                self.metrics.reset_start_time();
                                fetch_block_by_hash(effect_builder, peer, block_hash)
                            }
                        }
                    }
                    BlockByHashResult::FromStorage(block) => {
                        // We shouldn't get invalid data from the storage.
                        // If we do, it's a bug.
                        assert_eq!(*block.hash(), block_hash, "Block hash mismatch.");
                        trace!(%block_hash, "Linear block found in the local storage.");
                        // We hit a block that we already had in the storage - which should mean
                        // that we also have all of its ancestors, so we switch to traversing the
                        // chain forwards and downloading the deploys.
                        // We don't want to download and execute a block we already have, so
                        // instead of calling self.block_downloaded(), we take a shortcut:
                        self.set_last_block_if_syncing_trusted_hash(&block);
                        self.block_handled(rng, effect_builder, *block)
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
                    event::DeploysResult::Found(block) => {
                        let block_hash = block.hash();
                        trace!(%block_hash, "deploys for linear chain block found");
                        // Reset used peers so we can download next block with the full set.
                        self.peers.reset(rng);
                        // Execute block
                        let finalized_block: FinalizedBlock = (*block).into();
                        effect_builder.execute_block(finalized_block).ignore()
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
                                fetch_block_deploys(effect_builder, peer, *block)
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
                self.peers.push(peer_id.clone());

                let effects = if !self.started {
                    (async move {
                        match operations::run_fast_sync_task(effect_builder, init_hash).await {
                            Ok(()) => Some(Event::Start(peer_id)),
                            Err(error) => {
                                effect_builder
                                    .fatal(file!(), line!(), format!("{:?}", error))
                                    .await;
                                None
                            }
                        }
                    })
                    .map_some(std::convert::identity)
                } else {
                    Effects::new()
                };
                self.started = true;

                effects
            }

            Event::BlockHandled(block) => {
                let block_height = block.height();
                let block_hash = *block.hash();
                let effects = self.block_handled(rng, effect_builder, *block);
                trace!(%block_height, %block_hash, "block handled");
                effects
            }
            Event::GotUpgradeActivationPoint(next_upgrade_activation_point) => {
                trace!(?next_upgrade_activation_point, "new activation point");
                self.next_upgrade_activation_point = Some(next_upgrade_activation_point);
                Effects::new()
            }
            Event::InitUpgradeShutdown => {
                info!("shutdown initiated");
                // Serialize and store state.
                self.handle_upgrade_shutdown(effect_builder)
            }
            Event::Shutdown(upgrade) => {
                info!(?upgrade, "ready for shutdown");
                self.stop_for_upgrade = upgrade;
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

fn fetch_block_deploys<I: Clone + Send + 'static, REv>(
    effect_builder: EffectBuilder<REv>,
    peer: I,
    block: Block,
) -> Effects<Event<I>>
where
    REv: ReactorEventT<I>,
{
    let block_timestamp = block.header().timestamp();
    effect_builder
        .validate_block(peer.clone(), block, block_timestamp)
        .event(move |(found, block)| {
            if found {
                Event::GetDeploysResult(DeploysResult::Found(Box::new(block)))
            } else {
                Event::GetDeploysResult(DeploysResult::NotFound(Box::new(block), peer))
            }
        })
}

fn fetch_block_by_hash<I: Clone + Send + 'static, REv>(
    effect_builder: EffectBuilder<REv>,
    peer: I,
    block_hash: BlockHash,
) -> Effects<Event<I>>
where
    REv: ReactorEventT<I>,
{
    let cloned = peer.clone();
    effect_builder.fetch_block(block_hash, peer).map_or_else(
        move |fetch_result| match fetch_result {
            FetchResult::FromStorage(block) => {
                Event::GetBlockHashResult(block_hash, BlockByHashResult::FromStorage(block))
            }
            FetchResult::FromPeer(block, peer) => {
                Event::GetBlockHashResult(block_hash, BlockByHashResult::FromPeer(block, peer))
            }
        },
        move || Event::GetBlockHashResult(block_hash, BlockByHashResult::Absent(cloned)),
    )
}

fn fetch_block_at_height<I: Send + Clone + 'static, REv>(
    effect_builder: EffectBuilder<REv>,
    peer: I,
    block_height: u64,
) -> Effects<Event<I>>
where
    REv: ReactorEventT<I>,
{
    let cloned = peer.clone();
    effect_builder
        .fetch_block_by_height(block_height, peer.clone())
        .map_or_else(
            move |fetch_result| match fetch_result {
                FetchResult::FromPeer(result, _) => match *result {
                    BlockByHeight::Absent(ret_height) => {
                        warn!(
                            "Fetcher returned result for invalid height. Expected {}, got {}",
                            block_height, ret_height
                        );
                        Event::GetBlockHeightResult(block_height, BlockByHeightResult::Absent(peer))
                    }
                    BlockByHeight::Block(block) => Event::GetBlockHeightResult(
                        block_height,
                        BlockByHeightResult::FromPeer(block, peer),
                    ),
                },
                FetchResult::FromStorage(result) => match *result {
                    BlockByHeight::Absent(_) => {
                        // Fetcher should try downloading the block from a peer
                        // when it can't find it in the storage.
                        panic!("Should not return `Absent` in `FromStorage`.")
                    }
                    BlockByHeight::Block(block) => Event::GetBlockHeightResult(
                        block_height,
                        BlockByHeightResult::FromStorage(block),
                    ),
                },
            },
            move || Event::GetBlockHeightResult(block_height, BlockByHeightResult::Absent(cloned)),
        )
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
    bincode::deserialize(&serialized_state).unwrap_or_else(|error| {
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
pub(crate) fn read_init_state(
    storage: &Storage,
    chainspec: &Chainspec,
) -> Result<Option<State>, storage::Error> {
    let key = create_state_key(&chainspec);
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
    let key = create_state_key(&chainspec);
    storage.del_state_store(key)
}
