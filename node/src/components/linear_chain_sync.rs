//! Linear chain synchronizer.
//!
//! Synchronizes the linear chain when node joins the network.
//!
//! Steps are:
//! 1. Fetch blocks up to initial, trusted hash.
//! 2. Fetch deploys up to the trusted hash.
//! 3. Execute blocks (validate correctness – TBD).
//! 4. Transition to `SyncingDescendants` state.
//! 5. Fetch child block of trusted hash (it's the block with the highest `block_height`).
//! 6. Fetch deploys of that block.
//! 7. Execute the block (validate correctness – TBD).
//! 8. Repeat steps 4-7 as long as there's a child in the linear chain.
//!
//! The order of "download block – download deploys – execute" block steps differ,
//! in order to increase the chances of catching up with the linear chain quicker.
//! When synchronizing linear chain up to the trusted hash we cannot execute later blocks without
//! earlier ones. When we're syncing descendants, on the other hand, we can and we want to do it
//! ASAP so that we can start participating in consensus. That's why deploy fetching and block
//! execution is interleaved. If we had downloaded the whole chain, and then deploys, and then
//! execute (as we do in the first, SynchronizeTrustedHash, phase) it would have taken more time and
//! we might miss more eras.

mod event;

use super::{fetcher::FetchResult, storage::Storage, Component};
use crate::{
    effect::{self, EffectBuilder, EffectExt, EffectOptionExt, Effects},
    types::{Block, BlockByHeight, BlockHash, FinalizedBlock},
};
use effect::requests::{
    BlockExecutorRequest, BlockValidationRequest, FetcherRequest, StorageRequest,
};
use event::BlockByHeightResult;
pub use event::Event;
use rand::{seq::SliceRandom, CryptoRng, Rng};
use std::{collections::VecDeque, fmt::Display, mem};
use tracing::{error, info, trace, warn};

pub trait ReactorEventT<I>:
    From<StorageRequest<Storage>>
    + From<FetcherRequest<I, Block>>
    + From<FetcherRequest<I, BlockByHeight>>
    + From<BlockValidationRequest<Block, I>>
    + From<BlockExecutorRequest>
    + Send
{
}

impl<I, REv> ReactorEventT<I> for REv where
    REv: From<StorageRequest<Storage>>
        + From<FetcherRequest<I, Block>>
        + From<FetcherRequest<I, BlockByHeight>>
        + From<BlockValidationRequest<Block, I>>
        + From<BlockExecutorRequest>
        + Send
{
}

#[derive(Debug)]
enum State {
    // No syncing of the linear chain configured.
    None,
    // Synchronizing the linear chain up until trusted hash.
    SyncingTrustedHash {
        // Linear chain block to start sync from.
        trusted_hash: BlockHash,
        // During synchronization we might see new eras being created.
        // Track the highest height and wait until it's handled by consensus.
        highest_block_seen: u64,
        // Chain of downloaded blocks from the linear chain.
        // We will `pop()` when executing blocks.
        linear_chain: Vec<Block>,
    },
    // Synchronizing the descendants of the trusted hash.
    SyncingDescendants {
        trusted_hash: BlockHash,
        // Chain of downloaded blocks from the linear chain.
        linear_chain: VecDeque<Block>,
        // During synchronization we might see new eras being created.
        // Track the highest height and wait until it's handled by consensus.
        highest_block_seen: u64,
    },
    // Synchronizing done.
    Done,
}

impl State {
    fn sync_trusted_hash(trusted_hash: BlockHash) -> Self {
        State::SyncingTrustedHash {
            trusted_hash,
            highest_block_seen: 0,
            linear_chain: Vec::new(),
        }
    }

    fn sync_descendants(trusted_hash: BlockHash) -> Self {
        State::SyncingDescendants {
            trusted_hash,
            linear_chain: VecDeque::new(),
            highest_block_seen: 0,
        }
    }

    fn block_downloaded(&mut self, block: &Block) {
        match self {
            State::None | State::Done => {}
            State::SyncingTrustedHash {
                highest_block_seen, ..
            }
            | State::SyncingDescendants {
                highest_block_seen, ..
            } => {
                let curr_height = block.height();
                if curr_height > *highest_block_seen {
                    *highest_block_seen = curr_height;
                }
            }
        };
    }

    fn is_done(&self) -> bool {
        match self {
            State::Done => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub(crate) struct LinearChainSync<I> {
    // Set of peers that we can requests block from.
    peers: Vec<I>,
    // Peers we have not yet requested current block from.
    // NOTE: Maybe use a bitmask to decide which peers were tried?.
    peers_to_try: Vec<I>,
    state: State,
}

impl<I: Clone + PartialEq + 'static> LinearChainSync<I> {
    #[allow(unused)]
    pub fn new<REv: ReactorEventT<I>>(
        effect_builder: EffectBuilder<REv>,
        init_hash: Option<BlockHash>,
    ) -> Self {
        let state = init_hash.map_or(State::None, State::sync_trusted_hash);
        LinearChainSync {
            peers: Vec::new(),
            peers_to_try: Vec::new(),
            state,
        }
    }

    /// Resets `peers_to_try` back to all `peers` we know of.
    fn reset_peers<R: Rng + ?Sized>(&mut self, rng: &mut R) {
        self.peers_to_try = self.peers.clone();
        self.peers_to_try.as_mut_slice().shuffle(rng);
    }

    /// Returns a random peer.
    fn random_peer(&mut self) -> Option<I> {
        self.peers_to_try.pop()
    }

    // Unsafe version of `random_peer`.
    // Panics if no peer is available for querying.
    fn random_peer_unsafe(&mut self) -> I {
        self.random_peer().expect("At least one peer available.")
    }

    // Peer misbehaved (returned us invalid data).
    // Remove it from the set of nodes we request data from.
    fn ban_peer(&mut self, peer: I) {
        let index = self.peers.iter().position(|p| *p == peer);
        index.map(|idx| self.peers.remove(idx));
    }

    /// Add new block to linear chain.
    fn add_block(&mut self, block: Block) {
        match &mut self.state {
            State::None | State::Done => {}
            State::SyncingTrustedHash { linear_chain, .. } => linear_chain.push(block),
            State::SyncingDescendants { linear_chain, .. } => linear_chain.push_back(block),
        };
    }

    /// Returns `true` if we have finished syncing linear chain.
    pub fn is_synced(&self) -> bool {
        match self.state {
            State::None | State::Done => true,
            _ => false,
        }
    }

    /// Handles an event indicating that a linear chain block has been handled by consensus
    /// component.
    /// Returns effects that are created as a response to that event.
    fn block_handled<R, REv>(
        &mut self,
        rng: &mut R,
        effect_builder: EffectBuilder<REv>,
        block_height: u64,
    ) -> Effects<Event<I>>
    where
        I: Send + Copy + 'static,
        R: Rng + CryptoRng + ?Sized,
        REv: ReactorEventT<I>,
    {
        let curr_state = mem::replace(&mut self.state, State::None);
        let (new_state, mut effects) = match curr_state {
            State::None | State::Done => panic!("Block handled when in {:?} state.", &curr_state),
            State::SyncingTrustedHash {
                highest_block_seen,
                trusted_hash,
                ..
            } => {
                if block_height == highest_block_seen {
                    info!(%block_height, "Finished synchronizing linear chain up until trusted hash.");
                    self.reset_peers(rng);
                    let peer = self.random_peer_unsafe();
                    // Kick off syncing trusted hash descendants.
                    let effects = fetch_block_at_height(effect_builder, peer, block_height + 1);
                    (State::sync_descendants(trusted_hash), effects)
                } else {
                    (curr_state, Effects::new())
                }
            }
            State::SyncingDescendants {
                highest_block_seen, ..
            } => {
                if block_height == highest_block_seen {
                    info!(%block_height, "Finished synchronizing descendants of trusted hash.");
                    (State::Done, Effects::new())
                } else {
                    (curr_state, Effects::new())
                }
            }
        };
        self.state = new_state;
        if !self.state.is_done() {
            // Download next block deploys.
            let next_block_deploys_effect = self.fetch_next_block_deploys(effect_builder);
            effects.extend(next_block_deploys_effect);
        }
        effects
    }

    /// Returns effects for fetching next block's deploys.
    fn fetch_next_block_deploys<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event<I>>
    where
        I: Send + Copy + 'static,
        REv: ReactorEventT<I>,
    {
        let peer = self.random_peer_unsafe();

        let next_block = match self.state {
            State::None | State::Done => {
                panic!("Tried fetching next block when in {:?} state.", self.state)
            }
            State::SyncingTrustedHash {
                ref mut linear_chain,
                ..
            } => linear_chain.pop(),
            State::SyncingDescendants {
                ref mut linear_chain,
                ..
            } => linear_chain.pop_front(),
        };

        match next_block {
            None => {
                // We're done syncing but we have to wait for the execution of all blocks.
                Effects::new()
            }
            Some(block) => fetch_block_deploys(effect_builder, peer, block),
        }
    }
}

impl<I, REv, R> Component<REv, R> for LinearChainSync<I>
where
    I: Display + Clone + Copy + Send + PartialEq + 'static,
    R: Rng + CryptoRng + ?Sized,
    REv: ReactorEventT<I>,
{
    type Event = Event<I>;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Start(init_peer) => {
                match self.state {
                    State::None | State::Done | State::SyncingDescendants { .. } => {
                        // No syncing configured.
                        trace!("Received `Start` event when in {:?} state.", self.state);
                        Effects::new()
                    }
                    State::SyncingTrustedHash { trusted_hash, .. } => {
                        trace!(?trusted_hash, "Start synchronization");
                        // Start synchronization.
                        fetch_block_by_hash(effect_builder, init_peer, trusted_hash)
                    }
                }
            }
            Event::GetBlockHeightResult(block_height, fetch_result) => match fetch_result {
                BlockByHeightResult::Absent => match self.random_peer() {
                    None => {
                        // `block_height` not found on any of the peers.
                        // We have synchronized all, currently existing, descendants of trusted
                        // hash.
                        effect_builder
                            .immediately()
                            .event(move |_| Event::StartDownloadingDeploys)
                    }
                    Some(peer) => fetch_block_at_height(effect_builder, peer, block_height),
                },
                BlockByHeightResult::FromStorage(block) => {
                    self.state.block_downloaded(&block);
                    // We should be checking the local storage for linear blocks before we
                    // start syncing.
                    trace!(%block_height, "Linear block found in the local storage.");
                    // If we found the linear block in the storage it means we should have
                    // all of its parents as well. If that's not
                    // the case then we have a bug.
                    effect_builder
                        .immediately()
                        .event(move |_| Event::StartDownloadingDeploys)
                }
                BlockByHeightResult::FromPeer(block, peer) => {
                    if block.height() != block_height {
                        warn!(
                            "Block height mismatch. Expected {} got {} from {}.",
                            block_height,
                            block.height(),
                            peer
                        );
                        // NOTE: Signal misbehaving validator to networking layer.
                        self.ban_peer(peer);
                        return self.handle_event(
                            effect_builder,
                            rng,
                            Event::GetBlockHeightResult(block_height, BlockByHeightResult::Absent),
                        );
                    }
                    self.state.block_downloaded(&block);
                    trace!(%block_height, "Downloaded linear chain block.");
                    let next_height = block.height() + 1;
                    self.add_block(*block);
                    self.reset_peers(rng);
                    let peer = self.random_peer_unsafe();
                    fetch_block_at_height(effect_builder, peer, next_height)
                }
            },
            Event::GetBlockHashResult(block_hash, fetch_result) => match fetch_result {
                None => match self.random_peer() {
                    None => {
                        error!(%block_hash, "Could not download linear block from any of the peers.");
                        panic!("Failed to download linear chain.")
                    }
                    Some(peer) => fetch_block_by_hash(effect_builder, peer, block_hash),
                },
                Some(FetchResult::FromStorage(block)) => {
                    self.state.block_downloaded(&block);
                    // We should be checking the local storage for linear blocks before we start
                    // syncing.
                    trace!(%block_hash, "Linear block found in the local storage.");
                    // If we found the linear block in the storage it means we should have all of
                    // its parents as well. If that's not the case then we have a bug.
                    effect_builder
                        .immediately()
                        .event(move |_| Event::StartDownloadingDeploys)
                }
                Some(FetchResult::FromPeer(block, peer)) => {
                    if *block.hash() != block_hash {
                        warn!(
                            "Block hash mismatch. Expected {} got {} from {}.",
                            block_hash,
                            block.hash(),
                            peer
                        );
                        // NOTE: Signal misbehaving validator to networking layer.
                        // NOTE: Cannot call `self.ban_peer` with `peer` value b/c it's fixed for
                        // `KeyFingerprint` type and we're abstract in what
                        // peer type is.
                        return self.handle_event(
                            effect_builder,
                            rng,
                            Event::GetBlockHashResult(block_hash, None),
                        );
                    }
                    self.state.block_downloaded(&block);
                    trace!(%block_hash, "Downloaded linear chain block.");
                    self.add_block(*block.clone());

                    if block.is_genesis_child() {
                        info!("Linear chain downloaded. Start downloading deploys.");
                        effect_builder
                            .immediately()
                            .event(move |_| Event::StartDownloadingDeploys)
                    } else {
                        self.reset_peers(rng);
                        let parent_hash = *block.parent_hash();
                        let peer = self.random_peer_unsafe();
                        fetch_block_by_hash(effect_builder, peer, parent_hash)
                    }
                }
            },
            Event::DeploysFound(block) => {
                let block_height = block.height();
                trace!(%block_height, "Deploys for linear chain block found.");
                // Reset used peers so we can download next block with the full set.
                self.reset_peers(rng);
                // Execute block
                let finalized_block: FinalizedBlock = (*block).into();
                effect_builder.execute_block(finalized_block).ignore()
            }
            Event::DeploysNotFound(block) => match self.random_peer() {
                None => {
                    let block_hash = block.hash();
                    error!(%block_hash, "Could not download deploys from linear chain block.");
                    panic!("Failed to download linear chain deploys.")
                }
                Some(peer) => fetch_block_deploys(effect_builder, peer, *block),
            },
            Event::StartDownloadingDeploys => {
                // Start downloading deploys from the first block of the linear chain.
                self.reset_peers(rng);
                self.fetch_next_block_deploys(effect_builder)
            }
            Event::NewPeerConnected(peer_id) => {
                trace!(%peer_id, "New peer connected");
                // Add to the set of peers we can request things from.
                let mut effects = Effects::new();
                if self.peers.is_empty() {
                    // First peer connected, start downloading.
                    effects.extend(
                        effect_builder
                            .immediately()
                            .event(move |_| Event::Start(peer_id)),
                    );
                }
                self.peers.push(peer_id);
                effects
            }
            Event::BlockHandled(block_height) => {
                trace!(?block_height, "Block handled.");
                self.block_handled(rng, effect_builder, block_height)
            }
        }
    }
}

fn fetch_block_deploys<I: Send + Copy + 'static, REv>(
    effect_builder: EffectBuilder<REv>,
    peer: I,
    block: Block,
) -> Effects<Event<I>>
where
    REv: ReactorEventT<I>,
{
    effect_builder
        .validate_block(peer, block)
        .event(move |(found, block)| {
            if found {
                Event::DeploysFound(Box::new(block))
            } else {
                Event::DeploysNotFound(Box::new(block))
            }
        })
}

fn fetch_block_by_hash<I: Send + Copy + 'static, REv>(
    effect_builder: EffectBuilder<REv>,
    peer: I,
    block_hash: BlockHash,
) -> Effects<Event<I>>
where
    REv: ReactorEventT<I>,
{
    effect_builder.fetch_block(block_hash, peer).option(
        move |value| Event::GetBlockHashResult(block_hash, Some(value)),
        move || Event::GetBlockHashResult(block_hash, None),
    )
}

fn fetch_block_at_height<I: Send + Copy + 'static, REv>(
    effect_builder: EffectBuilder<REv>,
    peer: I,
    block_height: u64,
) -> Effects<Event<I>>
where
    REv: ReactorEventT<I>,
{
    effect_builder
        .fetch_block_by_height(block_height, peer)
        .option(
            move |fetch_result| match fetch_result {
                FetchResult::FromPeer(result, _) => match *result {
                    BlockByHeight::Absent(ret_height) => {
                        warn!(
                            "Fetcher returned result for invalid height. Expected {}, got {}",
                            block_height, ret_height
                        );
                        Event::GetBlockHeightResult(block_height, BlockByHeightResult::Absent)
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
            move || Event::GetBlockHeightResult(block_height, BlockByHeightResult::Absent),
        )
}
