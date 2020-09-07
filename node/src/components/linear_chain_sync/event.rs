use crate::{
    components::fetcher::FetchResult,
    types::{Block, BlockHash, DeployHash},
};
use std::fmt::Display;

#[derive(Debug)]
pub enum Event {
    Start(BlockHash),
    GetBlockResult(BlockHash, Option<FetchResult<Block>>),
    DeployFound(DeployHash),
    DeployNotFound(DeployHash),
    LinearChainBlocksDownloaded(),
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Start(block_hash) => write!(f, "Start syncing from {}.", block_hash),
            Event::GetBlockResult(bh, r) => write!(f, "Get block result for {}: {:?}", bh, r),
            Event::DeployFound(dh) => write!(f, "Deploy found: {}", dh),
            Event::DeployNotFound(dh) => write!(f, "Deploy mot found: {}", dh),
            Event::LinearChainBlocksDownloaded() => write!(f, "Linear chain blocks downloaded"),
        }
    }
}
