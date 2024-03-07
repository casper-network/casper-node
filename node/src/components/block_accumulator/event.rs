use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
};

use derive_more::From;

use casper_types::{BlockHash, BlockSignaturesV2, BlockV2, EraId, FinalitySignatureV2};

use crate::{
    effect::requests::BlockAccumulatorRequest,
    types::{ForwardMetaBlock, NodeId},
};

#[derive(Debug, From)]
pub(crate) enum Event {
    #[from]
    Request(BlockAccumulatorRequest),
    RegisterPeer {
        block_hash: BlockHash,
        era_id: Option<EraId>,
        sender: NodeId,
    },
    ReceivedBlock {
        block: Arc<BlockV2>,
        sender: NodeId,
    },
    CreatedFinalitySignature {
        finality_signature: Box<FinalitySignatureV2>,
    },
    ReceivedFinalitySignature {
        finality_signature: Box<FinalitySignatureV2>,
        sender: NodeId,
    },
    ExecutedBlock {
        meta_block: ForwardMetaBlock,
    },
    Stored {
        maybe_meta_block: Option<ForwardMetaBlock>,
        maybe_block_signatures: Option<BlockSignaturesV2>,
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
            Event::ExecutedBlock { meta_block } => {
                write!(f, "executed block {}", meta_block.block.hash())
            }
            Event::Stored {
                maybe_meta_block: Some(meta_block),
                maybe_block_signatures,
            } => {
                write!(
                    f,
                    "stored {} and {} finality signatures",
                    meta_block.block.hash(),
                    maybe_block_signatures
                        .as_ref()
                        .map(|sigs| sigs.len())
                        .unwrap_or_default()
                )
            }
            Event::Stored {
                maybe_meta_block: None,
                maybe_block_signatures,
            } => {
                write!(
                    f,
                    "stored {} finality signatures",
                    maybe_block_signatures
                        .as_ref()
                        .map(|sigs| sigs.len())
                        .unwrap_or_default()
                )
            }
        }
    }
}
