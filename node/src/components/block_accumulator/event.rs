use std::fmt::{self, Display, Formatter};

use derive_more::From;

use casper_types::EraId;

use crate::{
    effect::requests::BlockAccumulatorRequest,
    types::{Block, BlockHash, BlockSignatures, FinalitySignature, HotBlock, NodeId},
};

#[derive(Debug, From)]
pub(crate) enum Event {
    #[from]
    Request(BlockAccumulatorRequest),
    ValidatorMatrixUpdated,
    RegisterPeer {
        block_hash: BlockHash,
        era_id: Option<EraId>,
        sender: NodeId,
    },
    ReceivedBlock {
        block: Box<Block>,
        sender: NodeId,
    },
    CreatedFinalitySignature {
        finality_signature: Box<FinalitySignature>,
    },
    ReceivedFinalitySignature {
        finality_signature: Box<FinalitySignature>,
        sender: NodeId,
    },
    ExecutedBlock {
        hot_block: HotBlock,
    },
    Stored {
        maybe_hot_block: Option<HotBlock>,
        maybe_block_signatures: Option<BlockSignatures>,
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
            Event::RegisterPeer {
                block_hash, sender, ..
            } => {
                write!(
                    f,
                    "registering peer {} after gossip: {}",
                    sender, block_hash
                )
            }
            Event::ReceivedBlock { block, sender } => {
                write!(f, "received {} from {}", block, sender)
            }
            Event::CreatedFinalitySignature { finality_signature } => {
                write!(f, "created {}", finality_signature)
            }
            Event::ReceivedFinalitySignature {
                finality_signature,
                sender,
            } => {
                write!(f, "received {} from {}", finality_signature, sender)
            }
            Event::ExecutedBlock { hot_block } => {
                write!(f, "executed block {}", hot_block.block.hash())
            }
            Event::Stored {
                maybe_hot_block: Some(hot_block),
                maybe_block_signatures,
            } => {
                write!(
                    f,
                    "stored {} and {} finality signatures",
                    hot_block.block.hash(),
                    maybe_block_signatures
                        .as_ref()
                        .map(|sigs| sigs.proofs.len())
                        .unwrap_or_default()
                )
            }
            Event::Stored {
                maybe_hot_block: None,
                maybe_block_signatures,
            } => {
                write!(
                    f,
                    "stored {} finality signatures",
                    maybe_block_signatures
                        .as_ref()
                        .map(|sigs| sigs.proofs.len())
                        .unwrap_or_default()
                )
            }
        }
    }
}
