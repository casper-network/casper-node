use thiserror::Error;

use casper_types::{crypto, EraId};

use crate::types::{BlockHash, BlockValidationError, NodeId};

#[derive(Error, Debug)]
pub enum InvalidGossipError {
    #[error("received cryptographically invalid block for: {block_hash} from: {peer} with error: {validation_error}")]
    Block {
        block_hash: BlockHash,
        peer: NodeId,
        validation_error: BlockValidationError,
    },
    #[error("received cryptographically invalid finality_signature for: {block_hash} from: {peer} with error: {validation_error}")]
    FinalitySignature {
        block_hash: BlockHash,
        peer: NodeId,
        validation_error: crypto::Error,
    },
}

impl InvalidGossipError {
    pub(super) fn peer(&self) -> NodeId {
        match self {
            InvalidGossipError::FinalitySignature { peer, .. }
            | InvalidGossipError::Block { peer, .. } => *peer,
        }
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    InvalidGossip(Box<InvalidGossipError>),
    #[error("invalid configuration")]
    InvalidConfiguration,
    #[error("mismatched eras detected")]
    EraMismatch {
        block_hash: BlockHash,
        expected: EraId,
        actual: EraId,
        peer: NodeId,
    },
    #[error("mismatched block hash: expected={expected}, actual={actual}")]
    BlockHashMismatch {
        expected: BlockHash,
        actual: BlockHash,
    },
    #[error("should not be possible to have sufficient finality wihtout block: {block_hash}")]
    SufficientFinalityWithoutBlock { block_hash: BlockHash },
}
