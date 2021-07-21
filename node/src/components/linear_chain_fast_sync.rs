//! Fast linear chain synchronizer.
mod event;
mod metrics;
mod peers;
mod state;
mod traits;

use std::{collections::BTreeMap, convert::Infallible, fmt::Display, mem};

use datasize::DataSize;
use prometheus::Registry;
use tracing::{debug, error, info, trace, warn};

use casper_types::{PublicKey, U512};

use self::event::{BlockByHashResult, DeploysResult};

use super::{
    fetcher::FetchResult,
    storage::{self, Storage},
    Component,
};
use crate::{
    effect::{EffectBuilder, EffectExt, EffectOptionExt, Effects},
    types::{
        ActivationPoint, Block, BlockByHeight, BlockHash, BlockHeader, Chainspec, FinalizedBlock,
    },
    NodeRng,
};
use event::BlockByHeightResult;
pub use event::Event;
pub use metrics::LinearChainSyncMetrics;
pub use peers::PeersState;
pub use state::State;
pub use traits::ReactorEventT;

#[derive(DataSize, Debug)]
pub(crate) struct LinearChainFastSync<I> {
    peers: PeersState<I>,
    state: State,
    #[data_size(skip)]
    metrics: LinearChainSyncMetrics,
}

#[allow(dead_code)]
impl<I: Clone + PartialEq + 'static> LinearChainFastSync<I> {
    #[allow(clippy::too_many_arguments)]
    pub fn new<REv, Err>(
        registry: &Registry,
        _effect_builder: EffectBuilder<REv>,
        _chainspec: &Chainspec,
        _storage: &Storage,
        init_hash: Option<BlockHash>,
        _highest_block: Option<Block>,
        genesis_validator_weights: BTreeMap<PublicKey, U512>,
        _next_upgrade_activation_point: Option<ActivationPoint>,
    ) -> Result<(Self, Effects<Event<I>>), Err>
    where
        Err: From<prometheus::Error> + From<storage::Error>,
    {
        let no_effects = Effects::new();
        let state = init_hash.map_or(State::None, |init_hash| {
            State::sync_trusted_hash(init_hash, genesis_validator_weights)
        });
        let fast_sync = LinearChainFastSync {
            peers: PeersState::new(),
            state,
            metrics: LinearChainSyncMetrics::new(registry)?,
        };
        Ok((fast_sync, no_effects))
    }

    /// Add new block to linear chain.
    fn add_block(&mut self, block: Block) {
        match &mut self.state {
            State::None | State::Done => {}
            State::SyncingTrustedHash { linear_chain, .. } => linear_chain.push(block),
            State::SyncingDescendants { latest_block, .. } => **latest_block = block,
        };
    }

    /// Returns `true` if we have finished syncing linear chain.
    pub fn is_synced(&self) -> bool {
        matches!(self.state, State::None | State::Done)
    }

    /// Fast sync won't shut down for an upgrade.
    pub fn stopped_for_upgrade(&self) -> bool {
        false
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
        self.state.block_downloaded(block.header());
        self.add_block(block.clone());
        match &mut self.state {
            State::None | State::Done => panic!("Downloaded block when in {} state.", self.state),
            State::SyncingTrustedHash {
                trusted_hash,
                trusted_header,
                ..
            } => {
                if *block.hash() == *trusted_hash {
                    *trusted_header = Some(Box::new(block.header().clone()));
                }
                if block.header().is_genesis_child() {
                    info!("linear chain downloaded. Start downloading deploys.");
                    effect_builder
                        .immediately()
                        .event(move |_| Event::StartDownloadingDeploys)
                } else {
                    self.fetch_next_block(effect_builder, rng, block.header())
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
        self.state = State::Done;
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
        REv: ReactorEventT<I>,
    {
        let height = block.height();
        let hash = block.hash();
        trace!(%hash, %height, "Downloaded linear chain block.");
        // Reset peers before creating new requests.
        self.peers.reset(rng);
        let block_height = block.height();
        let mut curr_state = mem::replace(&mut self.state, State::None);
        match curr_state {
            State::None | State::Done => panic!("Block handled when in {:?} state.", &curr_state),
            State::SyncingTrustedHash {
                highest_block_seen,
                trusted_header: None,
                ..
            } if highest_block_seen == block_height => panic!("Should always have trusted header"),
            // If the block we are handling is the highest block seen, transition to syncing
            // descendants
            State::SyncingTrustedHash {
                highest_block_seen,
                trusted_hash,
                trusted_header,
                ref latest_block,
                validator_weights,
                ..
            } if highest_block_seen == block_height => {
                // TODO: Fail gracefully in these cases
                match latest_block.as_ref() {
                    Some(expected) => assert_eq!(
                        expected, &block,
                        "Block execution result doesn't match received block."
                    ),
                    None => panic!("Unexpected block execution results."),
                }
                let trusted_header = trusted_header.expect("trusted header must be present");

                info!(%block_height, "Finished synchronizing linear chain up until trusted hash.");
                let peer = self.peers.random_unsafe();
                // Kick off syncing trusted hash descendants.
                self.state =
                    State::sync_descendants(trusted_hash, trusted_header, block, validator_weights);
                fetch_block_at_height(effect_builder, peer, block_height + 1)
            }
            // Keep syncing from genesis if we haven't reached the trusted block hash
            State::SyncingTrustedHash {
                ref mut validator_weights,
                ref latest_block,
                ..
            } => {
                match latest_block.as_ref() {
                    Some(expected) => assert_eq!(
                        expected, &block,
                        "Block execution result doesn't match received block."
                    ),
                    None => panic!("Unexpected block execution results."),
                }
                if let Some(validator_weights_for_new_era) =
                    block.header().next_era_validator_weights()
                {
                    *validator_weights = validator_weights_for_new_era.clone();
                }
                self.state = curr_state;
                self.fetch_next_block_deploys(effect_builder)
            }
            State::SyncingDescendants {
                ref latest_block,
                ref mut validators_for_latest_block,
                ..
            } => {
                assert_eq!(
                    **latest_block, block,
                    "Block execution result doesn't match received block."
                );
                match block.header().next_era_validator_weights() {
                    None => (),
                    Some(validators_for_next_era) => {
                        *validators_for_latest_block = validators_for_next_era.clone();
                    }
                }
                self.state = curr_state;
                self.fetch_next_block(effect_builder, rng, block.header())
            }
        }
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
            State::None | State::Done => {
                panic!("Tried fetching next block when in {:?} state.", self.state)
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
        block_header: &BlockHeader,
    ) -> Effects<Event<I>>
    where
        I: Send + 'static,
        REv: ReactorEventT<I>,
    {
        self.peers.reset(rng);
        let peer = self.peers.random_unsafe();
        match self.state {
            State::SyncingTrustedHash { .. } => {
                let parent_hash = *block_header.parent_hash();
                self.metrics.reset_start_time();
                fetch_block_by_hash(effect_builder, peer, parent_hash)
            }
            State::SyncingDescendants { .. } => {
                let next_height = block_header.height() + 1;
                self.metrics.reset_start_time();
                fetch_block_at_height(effect_builder, peer, next_height)
            }
            State::Done | State::None => {
                panic!("Tried fetching block when in {:?} state", self.state)
            }
        }
    }

    pub(crate) fn latest_block(&self) -> Option<&Block> {
        match &self.state {
            State::SyncingTrustedHash { latest_block, .. } => Option::as_ref(&*latest_block),
            State::SyncingDescendants { latest_block, .. } => Some(&*latest_block),
            State::Done | State::None => None,
        }
    }
}

impl<I, REv> Component<REv> for LinearChainFastSync<I>
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
                match self.state {
                    State::None => {
                        // No syncing configured.
                        trace!("received `Start` event when in {} state.", self.state);
                        Effects::new()
                    }
                    State::Done | State::SyncingDescendants { .. } => {
                        // Illegal states for syncing start.
                        error!(
                            "should not have received `Start` event when in {} state.",
                            self.state
                        );
                        Effects::new()
                    }
                    State::SyncingTrustedHash { trusted_hash, .. } => {
                        trace!(?trusted_hash, "start synchronization");
                        // Start synchronization.
                        self.metrics.reset_start_time();
                        fetch_block_by_hash(effect_builder, init_peer, trusted_hash)
                    }
                }
            }
            Event::GetBlockHeightResult(block_height, fetch_result) => {
                match fetch_result {
                    BlockByHeightResult::Absent(peer) => {
                        self.metrics.observe_get_block_by_height();
                        trace!(%block_height, %peer, "failed to download block by height. Trying next peer");
                        self.peers.failure(&peer);
                        match self.peers.random() {
                            None => {
                                // `block_height` not found on any of the peers.
                                // We have synchronized all, currently existing, descendants of
                                // trusted hash.
                                self.mark_done();
                                info!("finished synchronizing descendants of the trusted hash.");
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
                        // When syncing descendants of a trusted hash, we might have some of them in
                        // our local storage. If that's the case, just
                        // continue.
                        self.block_downloaded(rng, effect_builder, &*block)
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
                        self.block_downloaded(rng, effect_builder, &*block)
                    }
                }
            }
            Event::GetBlockHashResult(block_hash, fetch_result) => {
                match fetch_result {
                    BlockByHashResult::Absent(peer) => {
                        self.metrics.observe_get_block_by_hash();
                        trace!(%block_hash, %peer, "failed to download block by hash. Trying next peer");
                        self.peers.failure(&peer);
                        match self.peers.random() {
                            None => {
                                error!(%block_hash, "Could not download linear block from any of the peers.");
                                panic!("Failed to download linear chain.")
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
                        trace!(%block_hash, "linear block found in the local storage.");
                        self.block_downloaded(rng, effect_builder, &*block)
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
                        self.block_downloaded(rng, effect_builder, &*block)
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
                        trace!(%block_hash, %peer, "deploy for linear chain block not found. Trying next peer");
                        self.peers.failure(&peer);
                        match self.peers.random() {
                            None => {
                                error!(%block_hash,
                                "could not download deploys from linear chain block.");
                                panic!("Failed to download linear chain deploys.")
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
                let effects = self.block_handled(rng, effect_builder, *block);
                trace!(%block_height, %block_hash, "block handled");
                effects
            }
            Event::GotUpgradeActivationPoint(next_upgrade_activation_point) => {
                debug!(
                    ?next_upgrade_activation_point,
                    "new activation point ignored"
                );
                Effects::new()
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
    effect_builder
        .validate_block(peer.clone(), block.clone())
        .event(move |found| {
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
