use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
};

use datasize::DataSize;
use derive_more::From;

use casper_types::{Block, BlockV2, Deploy, DeployId, Transaction, TransactionId};

use crate::{
    components::consensus::{ClContext, ProposedBlock},
    effect::requests::DeployBufferRequest,
    types::FinalizedBlock,
};

#[derive(Debug, From, DataSize)]
pub(crate) enum Event {
    Initialize(Vec<Block>),
    #[from]
    Request(DeployBufferRequest),
    ReceiveTransactionGossiped(TransactionId),
    StoredTransaction(TransactionId, Option<Box<Transaction>>),
    BlockProposed(Box<ProposedBlock<ClContext>>),
    Block(Arc<BlockV2>),
    VersionedBlock(Arc<Block>),
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
            Event::ReceiveTransactionGossiped(deploy_id) => {
                write!(formatter, "receive deploy gossiped {}", deploy_id)
            }
            Event::StoredTransaction(deploy_id, maybe_deploy) => {
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
                    finalized_block.height
                )
            }
            Event::Block(_) => {
                write!(formatter, "block")
            }
            Event::VersionedBlock(_) => {
                write!(formatter, "versioned block")
            }
            Event::Expire => {
                write!(formatter, "expire deploys")
            }
        }
    }
}
