use crate::{
    components::fetcher::FetchResult,
    types::{Block, BlockHash},
};
use std::fmt::Display;

#[derive(Debug)]
pub enum Event {
    Start(BlockHash),
    GetBlockResult(BlockHash, Option<FetchResult<Block>>),
    /// Deploys from the block have been found.
    DeploysFound(Box<Block>),
    /// Deploys from the block have not been found.
    DeploysNotFound(Box<Block>),
    LinearChainBlocksDownloaded,
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Start(block_hash) => write!(f, "Start syncing from {}.", block_hash),
            Event::GetBlockResult(block_hash, r) => {
                write!(f, "Get block result for {}: {:?}", block_hash, r)
            }
            Event::DeploysFound(block) => write!(f, "Deploys for block found: {}", block.hash()),
            Event::DeploysNotFound(block_hash) => {
                write!(f, "Deploy for block found: {}", block_hash.hash())
            }
            Event::LinearChainBlocksDownloaded => write!(f, "Linear chain blocks downloaded"),
        }
    }
}
