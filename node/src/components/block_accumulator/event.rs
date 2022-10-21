use std::fmt::{self, Display, Formatter};

use derive_more::From;

use casper_types::EraId;

use crate::{
    effect::requests::BlockAccumulatorRequest,
    types::{Block, BlockHash, BlockHeader, FinalitySignature, FinalitySignatureId, NodeId},
};

#[derive(Debug, From)]
pub(crate) enum Event {
    #[from]
    Request(BlockAccumulatorRequest),
    ReceivedBlock {
        block: Box<Block>,
        sender: NodeId,
    },
    ReceivedFinalitySignature {
        finality_signature: Box<FinalitySignature>,
        sender: NodeId,
    },
    UpdatedValidatorMatrix {
        era_id: EraId,
    },
    ExecutedBlock {
        block_header: BlockHeader,
    },
    Stored {
        block_hash: Option<BlockHash>,
        finality_signature_ids: Vec<FinalitySignatureId>,
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
            Event::ReceivedBlock { block, sender } => {
                write!(f, "received {} from {}", block, sender)
            }
            Event::ReceivedFinalitySignature {
                finality_signature,
                sender,
            } => {
                write!(f, "received {} from {}", finality_signature, sender)
            }
            Event::UpdatedValidatorMatrix { era_id } => {
                write!(f, "validator matrix update for era {}", era_id)
            }
            Event::ExecutedBlock { block_header } => {
                write!(f, "executed block: hash={}", block_header.block_hash())
            }
            Event::Stored {
                block_hash,
                finality_signature_ids,
            } => {
                write!(
                    f,
                    "stored {:?} and {} finality signatures",
                    block_hash,
                    finality_signature_ids.len()
                )
            }
        }
    }
}
