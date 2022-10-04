use thiserror::Error;

use casper_types::{crypto, EraId};

use crate::types::{ExecutedBlockValidationError, BlockHash, NodeId};

#[derive(Error, Debug)]
pub(super) enum EraMismatchError {
    #[error("attempt to add block: {block_hash} with mismatched era; expected: {expected} actual: {actual}")]
    Block {
        block_hash: BlockHash,
        expected: EraId,
        actual: EraId,
    },
    #[error("attempt to add finality signature for block: {block_hash} with mismatched era; expected: {expected} actual: {actual}")]
    FinalitySignature {
        block_hash: BlockHash,
        expected: EraId,
        actual: EraId,
    },
    #[error("attempt to add era validator weights to validate block: {block_hash} with mismatched era; expected: {expected} actual: {actual}")]
    EraValidatorWeights {
        block_hash: BlockHash,
        expected: EraId,
        actual: EraId,
    },
}

impl EraMismatchError {
    pub(super) fn block_hash(&self) -> BlockHash {
        match self {
            EraMismatchError::Block { block_hash, .. }
            | EraMismatchError::FinalitySignature { block_hash, .. }
            | EraMismatchError::EraValidatorWeights { block_hash, .. } => *block_hash,
        }
    }
}

#[derive(Error, Debug)]
pub(super) enum InvalidGossipError {
    #[error("received cryptographically invalid block_added for: {block_hash} from: {peer} with error: {validation_error}")]
    BlockAdded {
        block_hash: BlockHash,
        peer: NodeId,
        validation_error: ExecutedBlockValidationError,
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
            InvalidGossipError::BlockAdded { peer, .. }
            | InvalidGossipError::FinalitySignature { peer, .. } => *peer,
        }
    }
}

#[derive(Error, Debug)]
pub(super) enum Error {
    #[error(transparent)]
    InvalidGossip(Box<InvalidGossipError>),
    #[error("mismatched eras detected")]
    EraMismatch(EraMismatchError),
    #[error("attempt to register weights for an era that already has registered weights")]
    DuplicatedEraValidatorWeights { era_id: EraId },
}
