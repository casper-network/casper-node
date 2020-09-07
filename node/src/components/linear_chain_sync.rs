use super::{fetcher::FetchResult, storage::Storage, Component};
use crate::{
    effect::{self, EffectExt, EffectOptionExt, Effects},
    types::{Block, BlockHash, DeployHash},
};
use effect::requests::{FetcherRequest, StorageRequest};
use rand::{CryptoRng, Rng};
use std::fmt::Display;
use tracing::{error, info, warn};

#[derive(Debug)]
#[allow(unused)]
pub enum Event {
    Start(BlockHash),
    GetBlockResult(BlockHash, Option<FetchResult<Block>>),
    DeployFound(DeployHash),
    DeployNotFound(DeployHash),
    LinearChainBlocksDownloaded(),
}

struct LinearChainSync<I> {
    // Set of peers that we can requests blocks from.
    peers: Vec<I>,
    // Peers we have not yet requested current block from.
    peers_to_try: Vec<I>,
    // Chain of downloaded blocks from the linear chain.
    linear_chain: Vec<Block>,
    // Flag indicating whether we have finished syncing linear chain.
    is_synced: bool,
}

impl<I: Clone> LinearChainSync<I> {
    #[allow(unused)]
    pub fn new(peers: Vec<I>) -> Self {
        LinearChainSync {
            peers: peers.clone(),
            peers_to_try: peers,
            linear_chain: Vec::new(),
            is_synced: false,
        }
    }

    fn reset_peers(&mut self) {
        self.peers_to_try = self.peers.clone();
    }

    /// Returns `true` if we have finished syncing linear chain.
    #[allow(unused)]
    pub fn is_synced(&self) -> bool {
        self.is_synced
    }
}

impl<I, REv, R> Component<REv, R> for LinearChainSync<I>
where
    I: Display + Clone + Copy + Send + 'static,
    R: Rng + CryptoRng + ?Sized,
    REv: From<StorageRequest<Storage>> + From<FetcherRequest<I, Block>> + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: effect::EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Start(block_hash) => {
                let peer = self
                    .peers_to_try
                    .pop()
                    .expect("Should have at least 1 peer to start.");
                effect_builder.fetch_block(block_hash, peer).option(
                    move |value| Event::GetBlockResult(block_hash, Some(value)),
                    move || Event::GetBlockResult(block_hash, None),
                )
            }
            Event::GetBlockResult(block_hash, fetch_result) => match fetch_result {
                None => match self.peers_to_try.pop() {
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
                    info!("Linear block found in the local storage.");
                    // If we found the linear block in the storage it means we should have all of
                    // its parents as well. If that's not the case then we have a bug.
                    effect_builder
                        .immediately()
                        .event(move |_| Event::LinearChainBlocksDownloaded())
                }
                Some(FetchResult::FromPeer(block, peer)) => {
                    if *block.hash() != block_hash {
                        warn!(
                            "{} returned linear block where hash doesn't match {}",
                            peer, block_hash
                        );
                        // NOTE: Signal misbehaving validator to networking layer.
                        // Continue trying to fetch it from other peers.
                        // Panic for now.
                        panic!("Failed to download linear chain.")
                    }
                    self.linear_chain.push(*block.clone());
                    if block.is_genesis_child() {
                        info!("Linear chain downloaded. Starting downloading deploys.");
                        effect_builder
                            .put_block_to_storage(block)
                            .event(move |_| Event::LinearChainBlocksDownloaded())
                    } else {
                        self.reset_peers();
                        let parent_hash = *block.parent_hash();
                        let peer = self.peers_to_try.pop().expect("At least 1 peer available.");
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
            Event::DeployFound(_) => unimplemented!(),
            Event::DeployNotFound(_) => unimplemented!(),
            Event::LinearChainBlocksDownloaded() => {
                self.is_synced = true;
                Effects::new()
            }
        }
    }
}
