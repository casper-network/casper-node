use crate::types::{ActivationPoint, Block, BlockHash};

use casper_types::{PublicKey, U512};

use std::{
    collections::BTreeMap,
    fmt::{Debug, Display},
};

#[derive(Debug)]
pub enum Event<I> {
    Start(I),
    GotInitialValidators(BTreeMap<PublicKey, U512>),
    GetBlockHashResult(BlockHash, BlockByHashResult<I>),
    GetBlockHeightResult(u64, BlockByHeightResult<I>),
    GetDeploysResult(DeploysResult<I>),
    StartDownloadingDeploys,
    NewPeerConnected(I),
    BlockHandled(Box<Block>),
    GotUpgradeActivationPoint(ActivationPoint),
    InitUpgradeShutdown,
    Shutdown(bool),
}

#[derive(Debug)]
pub enum DeploysResult<I> {
    Found(Box<Block>),
    NotFound(Box<Block>, I),
}

#[derive(Debug)]
pub enum BlockByHashResult<I> {
    Absent(I),
    FromStorage(Box<Block>),
    FromPeer(Box<Block>, I),
}

#[derive(Debug)]
pub enum BlockByHeightResult<I> {
    Absent(I),
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
            Event::GotInitialValidators(validators) => {
                write!(f, "Got initial validators: {:?}", validators)
            }
            Event::GetBlockHashResult(block_hash, r) => {
                write!(f, "Get block result for {}: {:?}", block_hash, r)
            }
            Event::GetDeploysResult(result) => {
                write!(f, "Get deploys for block result {:?}", result)
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
            Event::GotUpgradeActivationPoint(activation_point) => {
                write!(f, "new upgrade activation point: {:?}", activation_point)
            }
            Event::InitUpgradeShutdown => write!(f, "shutdown for upgrade initiatied"),
            Event::Shutdown(upgrade) => write!(
                f,
                "linear chain sync is ready for shutdown. upgrade: {}",
                upgrade
            ),
        }
    }
}
