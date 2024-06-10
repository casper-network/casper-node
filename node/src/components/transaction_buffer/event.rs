use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
};

use datasize::DataSize;
use derive_more::From;

use casper_types::{Block, BlockV2, EraId, Timestamp, Transaction, TransactionId};

use crate::{
    components::consensus::{ClContext, ProposedBlock},
    effect::{requests::TransactionBufferRequest, Responder},
    types::{appendable_block::AppendableBlock, FinalizedBlock},
};

#[derive(Debug, From, DataSize)]
pub(crate) enum Event {
    Initialize(Vec<Block>),
    #[from]
    Request(TransactionBufferRequest),
    ReceiveTransactionGossiped(TransactionId),
    StoredTransaction(TransactionId, Option<Box<Transaction>>),
    BlockProposed(Box<ProposedBlock<ClContext>>),
    Block(Arc<BlockV2>),
    VersionedBlock(Arc<Block>),
    BlockFinalized(Box<FinalizedBlock>),
    Expire,
    UpdateEraGasPrice(EraId, u8),
    GetGasPriceResult(
        Option<u8>,
        EraId,
        Timestamp,
        Timestamp,
        Responder<AppendableBlock>,
    ),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Initialize(blocks) => {
                write!(formatter, "initialize, {} blocks", blocks.len())
            }
            Event::Request(TransactionBufferRequest::GetAppendableBlock { .. }) => {
                write!(formatter, "get appendable block request")
            }
            Event::ReceiveTransactionGossiped(transaction_id) => {
                write!(formatter, "receive transaction gossiped {}", transaction_id)
            }
            Event::StoredTransaction(transaction_id, maybe_transaction) => {
                write!(
                    formatter,
                    "{} stored: {:?}",
                    transaction_id,
                    maybe_transaction.is_some()
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
                write!(formatter, "expire transactions")
            }
            Event::UpdateEraGasPrice(era_id, next_era_gas_price) => {
                write!(
                    formatter,
                    "gas price {} for era {}",
                    next_era_gas_price, era_id
                )
            }
            Event::GetGasPriceResult(_, era_id, _, _, _) => {
                write!(formatter, "retrieving gas price for era {}", era_id)
            }
        }
    }
}
