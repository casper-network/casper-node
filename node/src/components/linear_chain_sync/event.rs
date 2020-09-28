use crate::{
    components::fetcher::FetchResult,
    types::{Block, BlockHash, BlockHeader},
};
use std::fmt::{Debug, Display};

#[derive(Debug)]
pub enum Event<I> {
    Start(I),
    GetBlockHashResult(BlockHash, Option<FetchResult<Block>>),
    GetBlockHeightResult(u64, BlockByHeightResult<I>),
    /// Deploys from the block have been found.
    DeploysFound(Box<BlockHeader>),
    /// Deploys from the block have not been found.
    DeploysNotFound(Box<BlockHeader>),
    StartDownloadingDeploys,
    NewPeerConnected(I),
    BlockHandled(Box<BlockHeader>),
}

#[derive(Debug)]
pub enum BlockByHeightResult<I> {
    Absent,
    FromStorage(Box<Block>),
    FromPeer(Box<Block>, I),
}

impl<I> Display for Event<I>
where
    I: Debug + Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Start(init_peer) => write!(f, "Start syncing from peer {}.", init_peer),
            Event::GetBlockHashResult(block_hash, r) => {
                write!(f, "Get block result for {}: {:?}", block_hash, r)
            }
            Event::DeploysFound(block) => write!(f, "Deploys for block found: {}", block.hash()),
            Event::DeploysNotFound(block_hash) => {
                write!(f, "Deploy for block found: {}", block_hash.hash())
            }
            Event::StartDownloadingDeploys => write!(f, "Start downloading deploys event."),
            Event::NewPeerConnected(peer_id) => write!(f, "A new peer connected: {}", peer_id),
            Event::BlockHandled(block) => {
                let hash = block.hash();
                let height = block.height();
                write!(
                    f,
                    "Block has been handled by consensus. Hash {}, height {}",
                    hash, height
                )
            }
            Event::GetBlockHeightResult(height, res) => {
                write!(f, "Get block result for height {}: {:?}", height, res)
            }
        }
    }
}
