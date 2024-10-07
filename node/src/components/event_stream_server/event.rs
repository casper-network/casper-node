use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
};

use crate::types::TransactionHeader;
use itertools::Itertools;

use casper_types::{
    contract_messages::Messages,
    execution::{Effects, ExecutionResult},
    Block, BlockHash, EraId, FinalitySignature, PublicKey, Timestamp, Transaction, TransactionHash,
};

#[derive(Debug)]
pub enum Event {
    Initialize,
    BlockAdded(Arc<Block>),
    TransactionAccepted(Arc<Transaction>),
    TransactionProcessed {
        transaction_hash: TransactionHash,
        transaction_header: Box<TransactionHeader>,
        block_hash: BlockHash,
        execution_result: Box<ExecutionResult>,
        messages: Messages,
    },
    TransactionsExpired(Vec<TransactionHash>),
    Fault {
        era_id: EraId,
        public_key: Box<PublicKey>,
        timestamp: Timestamp,
    },
    FinalitySignature(Box<FinalitySignature>),
    Step {
        era_id: EraId,
        execution_effects: Effects,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Event::Initialize => write!(formatter, "initialize"),
            Event::BlockAdded(block) => write!(formatter, "block added {}", block.hash()),
            Event::TransactionAccepted(transaction_hash) => {
                write!(formatter, "transaction accepted {}", transaction_hash)
            }
            Event::TransactionProcessed {
                transaction_hash, ..
            } => {
                write!(formatter, "transaction processed {}", transaction_hash)
            }
            Event::TransactionsExpired(transaction_hashes) => {
                write!(
                    formatter,
                    "transactions expired: {}",
                    transaction_hashes.iter().join(", ")
                )
            }
            Event::Fault {
                era_id,
                public_key,
                timestamp,
            } => write!(
                formatter,
                "An equivocator with public key: {} has been identified at time: {} in era: {}",
                public_key, timestamp, era_id,
            ),
            Event::FinalitySignature(fs) => write!(formatter, "finality signature {}", fs),
            Event::Step { era_id, .. } => write!(formatter, "step committed for {}", era_id),
        }
    }
}
