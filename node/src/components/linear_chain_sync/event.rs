use crate::{
    components::fetcher::FetchResult,
    types::{Block, BlockHash},
};
use std::fmt::Display;

#[derive(Debug)]
pub enum Event<I> {
    Start(I),
    GetBlockResult(BlockHash, Option<FetchResult<Block>>),
    /// Deploys from the block have been found.
    DeploysFound(Box<Block>),
    /// Deploys from the block have not been found.
    DeploysNotFound(Box<Block>),
    LinearChainBlocksDownloaded,
    NewPeerConnected(I),
    BlockExecutionDone(BlockHash, u64),
}

impl<I> Display for Event<I>
where
    I: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Start(init_peer) => write!(f, "Start syncing from peer {}.", init_peer),
            Event::GetBlockResult(block_hash, r) => {
                write!(f, "Get block result for {}: {:?}", block_hash, r)
            }
            Event::DeploysFound(block) => write!(f, "Deploys for block found: {}", block.hash()),
            Event::DeploysNotFound(block_hash) => {
                write!(f, "Deploy for block found: {}", block_hash.hash())
            }
            Event::LinearChainBlocksDownloaded => write!(f, "Linear chain blocks downloaded"),
            Event::NewPeerConnected(peer_id) => write!(f, "A new peer connected: {}", peer_id),
            Event::BlockExecutionDone(block_hash, height) => {
                write!(f, "Block execution done: {} @ {}", block_hash, height)
            }
        }
    }
}
