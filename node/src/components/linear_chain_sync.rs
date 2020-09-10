mod event;

use super::{fetcher::FetchResult, storage::Storage, Component};
use crate::{
    effect::{self, EffectBuilder, EffectExt, EffectOptionExt, Effects},
    types::{Block, BlockHash, FinalizedBlock},
};
use effect::requests::{
    BlockExecutorRequest, BlockValidationRequest, FetcherRequest, StorageRequest,
};
pub use event::Event;
use rand::{CryptoRng, Rng};
use std::fmt::Display;
use tracing::{error, info, trace, warn};

pub trait ReactorEventT<I>:
    From<StorageRequest<Storage>>
    + From<FetcherRequest<I, Block>>
    + From<BlockValidationRequest<Block, I>>
    + From<BlockExecutorRequest>
    + Send
{
}

impl<I, REv> ReactorEventT<I> for REv where
    REv: From<StorageRequest<Storage>>
        + From<FetcherRequest<I, Block>>
        + From<BlockValidationRequest<Block, I>>
        + From<BlockExecutorRequest>
        + Send
{
}

#[derive(Debug)]
pub(crate) struct LinearChainSync<I> {
    // Set of peers that we can requests block from.
    peers: Vec<I>,
    // Peers we have not yet requested current block from.
    // NOTE: Maybe use a bitmask to decide which peers were tried?.
    peers_to_try: Vec<I>,
    // Chain of downloaded blocks from the linear chain.
    linear_chain: Vec<Block>,
    // Flag indicating whether we have finished syncing linear chain.
    is_synced: bool,
    // Linear chain block to start sync from.
    init_hash: Option<BlockHash>,
}

impl<I: Clone + 'static> LinearChainSync<I> {
    #[allow(unused)]
    pub fn new<REv: ReactorEventT<I>>(
        effect_builder: EffectBuilder<REv>,
        init_hash: Option<BlockHash>,
    ) -> Self {
        LinearChainSync {
            peers: Vec::new(),
            peers_to_try: Vec::new(),
            linear_chain: Vec::new(),
            is_synced: init_hash.is_none(),
            init_hash,
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

    /// Returns `true` if we have finished syncing linear chain.
    pub fn is_synced(&self) -> bool {
        self.is_synced
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
        match self.linear_chain.pop() {
            None => {
                // We're done syncing
                self.is_synced = true;
                info!("Finished syncing linear chain.");
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
                match self.init_hash {
                    None => {
                        // No syncing configured.
                        Effects::new()
                    }
                    Some(init_hash) => {
                        // Start synchronization.
                        effect_builder.fetch_block(init_hash, init_peer).option(
                            move |value| Event::GetBlockResult(init_hash, Some(value)),
                            move || Event::GetBlockResult(init_hash, None),
                        )
                    }
                }
            }
            Event::GetBlockResult(block_hash, fetch_result) => match fetch_result {
                None => match self.random_peer(rng) {
                    None => {
                        error!(%block_hash, "Could not download linear block from any of the peers.");
                        panic!("Failed to download linear chain.")
                    }
                    Some(peer) => effect_builder.fetch_block(block_hash, peer).option(
                        move |value| Event::GetBlockResult(block_hash, Some(value)),
                        move || Event::GetBlockResult(block_hash, None),
                    ),
                },
                Some(FetchResult::FromStorage(_)) => {
                    // We should be checking the local storage for linear blocks before we start
                    // syncing.
                    trace!(%block_hash, "Linear block found in the local storage.");
                    // If we found the linear block in the storage it means we should have all of
                    // its parents as well. If that's not the case then we have a bug.
                    effect_builder
                        .immediately()
                        .event(move |_| Event::LinearChainBlocksDownloaded)
                }
                Some(FetchResult::FromPeer(block, peer)) => {
                    if *block.hash() != block_hash {
                        warn!(
                            "{} returned linear block where hash doesn't match {}",
                            peer, block_hash
                        );
                        // NOTE: Signal misbehaving validator to networking layer.
                        return self.handle_event(
                            effect_builder,
                            rng,
                            Event::GetBlockResult(*block.hash(), None),
                        );
                    }
                    trace!(%block_hash, "Downloaded linear chain block.");
                    self.linear_chain.push(*block.clone());
                    if block.is_genesis_child() {
                        info!("Linear chain downloaded. Starting downloading deploys.");
                        effect_builder
                            .put_block_to_storage(block)
                            .event(move |_| Event::LinearChainBlocksDownloaded)
                    } else {
                        self.reset_peers();
                        let parent_hash = *block.parent_hash();
                        let peer = self.random_peer_unsafe(rng);
                        let mut effects = effect_builder.put_block_to_storage(block).ignore();
                        let fetch_parent = effect_builder.fetch_block(parent_hash, peer).option(
                            move |value| Event::GetBlockResult(block_hash, Some(value)),
                            move || Event::GetBlockResult(block_hash, None),
                        );
                        effects.extend(fetch_parent);
                        effects
                    }
                }
            },
            Event::DeploysFound(block) => {
                let block_hash = block.hash();
                trace!(%block_hash, "Deploys for linear chain block found.");
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
            Event::LinearChainBlocksDownloaded => {
                // Start downloading deploys from the first block of the linear chain.
                self.fetch_next_block_deploys(effect_builder, rng)
            }
            Event::NewPeerConnected(peer_id) => {
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
