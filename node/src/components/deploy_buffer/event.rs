use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use derive_more::From;

use crate::{
    components::consensus::{ClContext, ProposedBlock},
    effect::requests::DeployBufferRequest,
    types::{Block, Deploy, DeployHash, FinalizedBlock},
};

#[derive(Debug, From, DataSize)]
pub(crate) enum Event {
    Initialize(Vec<Block>),
    #[from]
    Request(DeployBufferRequest),
    ReceiveDeploy(Box<Deploy>),
    BlockProposed(Box<ProposedBlock<ClContext>>),
    Block(Box<Block>),
    BlockFinalized(Box<FinalizedBlock>),
    DeployHashGossiped(DeployHash),
    Expire,
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Initialize(blocks) => {
                write!(formatter, "initialize, {} blocks", blocks.len())
            }
            Event::Request(DeployBufferRequest::GetAppendableBlock { .. }) => {
                write!(formatter, "get appendable block request")
            }
            Event::ReceiveDeploy(deploy) => {
                write!(formatter, "receive deploy {}", deploy.hash())
            }
            Event::BlockProposed(_) => {
                write!(formatter, "proposed block")
            }
            Event::BlockFinalized(finalized_block) => {
                write!(
                    formatter,
                    "finalized block at height {}",
                    finalized_block.height()
                )
            }
            Event::Block(_) => {
                write!(formatter, "block")
            }
            Event::Expire => {
                write!(formatter, "expire deploys")
            }
            Event::DeployHashGossiped(deploy_hash) => write!(
                formatter,
                "got notified about gossiped deploy hash={}",
                deploy_hash
            ),
        }
    }
}
