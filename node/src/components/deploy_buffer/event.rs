use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
};

use datasize::DataSize;
use derive_more::From;

use crate::{
    components::consensus::{ClContext, ProposedBlock},
    effect::requests::DeployBufferRequest,
    types::{Block, Deploy, DeployId, FinalizedBlock},
};

#[derive(Debug, From, DataSize)]
pub(crate) enum Event {
    Initialize(Vec<Block>),
    #[from]
    Request(DeployBufferRequest),
    ReceiveDeployGossiped(DeployId),
    StoredDeploy(DeployId, Box<Option<Deploy>>),
    BlockProposed(Box<ProposedBlock<ClContext>>),
    Block(Arc<Block>),
    BlockFinalized(Box<FinalizedBlock>),
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
            Event::ReceiveDeployGossiped(deploy_id) => {
                write!(formatter, "receive deploy gossiped {}", deploy_id)
            }
            Event::StoredDeploy(deploy_id, maybe_deploy) => {
                write!(
                    formatter,
                    "{} stored: {:?}",
                    deploy_id,
                    maybe_deploy.is_some()
                )
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
        }
    }
}
