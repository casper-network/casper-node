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
use rand::{CryptoRng, Rng};
use std::{fmt::Display, mem};
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
#[allow(unused)]
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
        // We will `pop()` when executing blocks.
        linear_chain: Vec<Block>,
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
            linear_chain: Vec::new(),
            highest_block_seen: 0,
        }
    }

    fn block_downloaded(&mut self, block: &Block) {
        match self {
            State::None | State::Done => {}
            State::SyncingTrustedHash {
                highest_block_seen, ..
            } => {
                let curr_height = block.height();
                // We instantiate with `highest_block_seen=0`, start downloading with the
                // highest block and then download its ancestors. It should
                // be updated only once at the start.
                if curr_height > *highest_block_seen {
                    *highest_block_seen = curr_height;
                }
            }
            State::SyncingDescendants {
                highest_block_seen, ..
            } => {
                let curr_height = block.height();
                if curr_height > *highest_block_seen {
                    *highest_block_seen = curr_height;
                }
            }
        };
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

impl<I: Clone + 'static> LinearChainSync<I> {
    #[allow(unused)]
    pub fn new<REv: ReactorEventT<I>>(
        effect_builder: EffectBuilder<REv>,
        init_hash: Option<BlockHash>,
    ) -> Self {
        let state = init_hash
            .map(State::sync_trusted_hash)
            .unwrap_or_else(|| State::None);
        LinearChainSync {
            peers: Vec::new(),
            peers_to_try: Vec::new(),
            state,
        }
    }

    fn reset_peers(&mut self) {
        self.peers_to_try = self.peers.clone();
    }

    fn random_peer<R: Rng + ?Sized>(&mut self, rand: &mut R) -> Option<I> {
        let peers_count = self.peers_to_try.len();
        if peers_count == 0 {
            return None;
        }
        if peers_count == 1 {
            return Some(self.peers_to_try.pop().expect("Not to fail"));
        }
        let idx = rand.gen_range(0, peers_count);
        Some(self.peers_to_try.remove(idx))
    }

    // Unsafe version of `random_peer`.
    // Panics if no peer is available for querying.
    fn random_peer_unsafe<R: Rng + ?Sized>(&mut self, rand: &mut R) -> I {
        self.random_peer(rand)
            .expect("At least one peer available.")
    }

    fn new_block(&mut self, block: Block) {
        match &mut self.state {
            State::None | State::Done => {}
            State::SyncingTrustedHash { linear_chain, .. }
            | State::SyncingDescendants { linear_chain, .. } => linear_chain.push(block),
        };
    }

    /// Returns `true` if we have finished syncing linear chain.
    pub fn is_synced(&self) -> bool {
        match self.state {
            State::None | State::Done => true,
            _ => false,
        }
    }

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
        let (new_state, effects) = match curr_state {
            State::None | State::Done => panic!("Block handled when in {:?} state.", &curr_state),
            State::SyncingTrustedHash {
                highest_block_seen,
                trusted_hash,
                ..
            } => {
                if block_height == highest_block_seen {
                    info!(%block_height, "Finished synchronizing linear chain up until trusted hash.");
                    self.reset_peers();
                    let peer = self.random_peer_unsafe(rng);
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
        effects
    }

    fn fetch_next_block_deploys<R, REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut R,
    ) -> Effects<Event<I>>
    where
        I: Send + Copy + 'static,
        R: Rng + CryptoRng + ?Sized,
        REv: ReactorEventT<I>,
    {
        let peer = self.random_peer_unsafe(rng);
        let next_block = match &mut self.state {
            State::None | State::Done => {
                panic!("Tried fetching next block when in {:?} state.", self.state)
            }
            State::SyncingTrustedHash { linear_chain, .. }
            | State::SyncingDescendants { linear_chain, .. } => linear_chain.pop(),
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
    I: Display + Clone + Copy + Send + 'static,
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
                    State::None | State::Done => {
                        // No syncing configured.
                        Effects::new()
                    }
                    State::SyncingTrustedHash { trusted_hash, .. } => {
                        trace!(?trusted_hash, "Start synchronization");
                        // Start synchronization.
                        fetch_block(effect_builder, init_peer, trusted_hash)
                    }
                    State::SyncingDescendants { .. } => {
                        panic!("`Start` event received when in `SyncingDescendants` state.")
                    }
                }
            }
            Event::GetBlockHeightResult(block_height, fetch_result) => match fetch_result {
                BlockByHeightResult::Absent => match self.random_peer(rng) {
                    None => {
                        // `block_height` not found on any of the peers.
                        // We have synchronized all, currently existing, descenandants of trusted
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
                    self.state.block_downloaded(&block);
                    if block.height() != block_height {
                        warn!(
                            "Block height mismatch. Expected {} got {} from {}.",
                            block_height,
                            block.height(),
                            peer
                        );
                        // NOTE: Signal misbehaving validator to networking layer.
                        return self.handle_event(
                            effect_builder,
                            rng,
                            Event::GetBlockHeightResult(block_height, BlockByHeightResult::Absent),
                        );
                    }
                    trace!(%block_height, "Downloaded linear chain block.");
                    self.reset_peers();
                    let next_height = block.height() + 1;
                    self.new_block(*block);
                    let peer = self.random_peer_unsafe(rng);
                    fetch_block_at_height(effect_builder, peer, next_height)
                }
            },
            Event::GetBlockHashResult(block_hash, fetch_result) => match fetch_result {
                None => match self.random_peer(rng) {
                    None => {
                        error!(%block_hash, "Could not download linear block from any of the peers.");
                        panic!("Failed to download linear chain.")
                    }
                    Some(peer) => fetch_block(effect_builder, peer, block_hash),
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
                    self.state.block_downloaded(&block);
                    if *block.hash() != block_hash {
                        warn!(
                            "Block hash mismatch. Expected {} got {} from {}.",
                            block_hash,
                            block.hash(),
                            peer
                        );
                        // NOTE: Signal misbehaving validator to networking layer.
                        return self.handle_event(
                            effect_builder,
                            rng,
                            Event::GetBlockHashResult(block_hash, None),
                        );
                    }
                    trace!(%block_hash, "Downloaded linear chain block.");
                    self.reset_peers();
                    self.new_block(*block.clone());
                    if block.is_genesis_child() {
                        info!("Linear chain downloaded. Starting downloading deploys.");
                        effect_builder
                            .immediately()
                            .event(move |_| Event::StartDownloadingDeploys)
                    } else {
                        let parent_hash = *block.parent_hash();
                        let peer = self.random_peer_unsafe(rng);
                        fetch_block(effect_builder, peer, parent_hash)
                    }
                }
            },
            Event::DeploysFound(block) => {
                let block_hash = *block.hash();
                trace!(%block_hash, "Deploys for linear chain block found.");
                // Reset used peers so we can download next block with the full set.
                self.reset_peers();
                // Execute block
                // Download next block deploys.
                let mut effects = self.fetch_next_block_deploys(effect_builder, rng);
                let finalized_block: FinalizedBlock = (*block).into();
                let execute_block_effect = effect_builder.execute_block(finalized_block).ignore();
                effects.extend(execute_block_effect);
                effects
            }
            Event::DeploysNotFound(block) => match self.random_peer(rng) {
                None => {
                    let block_hash = block.hash();
                    error!(%block_hash, "Could not download deploys from linear chain block.");
                    panic!("Failed to download linear chain deploys.")
                }
                Some(peer) => fetch_block_deploys(effect_builder, peer, *block),
            },
            Event::StartDownloadingDeploys => {
                // Start downloading deploys from the first block of the linear chain.
                self.reset_peers();
                self.fetch_next_block_deploys(effect_builder, rng)
            }
            Event::NewPeerConnected(peer_id) => {
                trace!(%peer_id, "New peer connected");
                let mut effects = Effects::new();
                if self.peers.is_empty() {
                    // First peer connected, start dowloading.
                    effects.extend(
                        effect_builder
                            .immediately()
                            .event(move |_| Event::Start(peer_id)),
                    );
                }
                // Add to the set of peers we can request things from.
                self.peers.push(peer_id);
                effects
            }
            Event::BlockHandled(height) => self.block_handled(rng, effect_builder, height),
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

fn fetch_block<I: Send + Copy + 'static, REv>(
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
            move |value| match value {
                FetchResult::FromPeer(result, _) => match *result {
                    BlockByHeight::Absent(_) => {
                        Event::GetBlockHeightResult(block_height, BlockByHeightResult::Absent)
                    }
                    BlockByHeight::Block(block) => Event::GetBlockHeightResult(
                        block_height,
                        BlockByHeightResult::FromPeer(block, peer),
                    ),
                },
                FetchResult::FromStorage(result) => match *result {
                    BlockByHeight::Absent(_) => {
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
