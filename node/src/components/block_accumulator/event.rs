use std::fmt::{self, Display, Formatter};

use derive_more::From;

use crate::{
    effect::requests::BlockAccumulatorRequest,
    types::{Block, FinalitySignature, NodeId},
};

#[derive(Debug, From)]
pub(crate) enum Event {
    #[from]
    Request(BlockAccumulatorRequest),
    ValidatorMatrixUpdated,
    ReceivedBlock {
        block: Box<Block>,
        sender: NodeId,
    },
    ReceivedFinalitySignature {
        finality_signature: Box<FinalitySignature>,
        sender: NodeId,
    },
    ExecutedBlock {
        block: Box<Block>,
        sender: NodeId,
    },
    Stored {
        block: Option<Box<Block>>,
        finality_signatures: Vec<FinalitySignature>,
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(BlockAccumulatorRequest::GetPeersForBlock { block_hash, .. }) => {
                write!(
                    f,
                    "block accumulator peers request for block: {}",
                    block_hash
                )
            }
            Event::ValidatorMatrixUpdated => {
                write!(f, "validator matrix updated")
            }
            Event::ReceivedBlock { block, sender } => {
                write!(f, "received {} from {}", block, sender)
            }
            Event::ReceivedFinalitySignature {
                finality_signature,
                sender,
            } => {
                write!(f, "received {} from {}", finality_signature, sender)
            }
            // Event::UpdatedValidatorMatrix { era_id } => {
            //     write!(f, "validator matrix update for era {}", era_id)
            // }
            Event::ExecutedBlock { block, sender } => {
                write!(f, "executed block: hash={} sender={}", block.hash(), sender)
            }
            Event::Stored {
                block,
                finality_signatures,
            } => {
                write!(
                    f,
                    "stored {:?} and {} finality signatures",
                    block.as_ref().map(|block| *block.hash()),
                    finality_signatures.len()
                )
            }
        }
    }
}
