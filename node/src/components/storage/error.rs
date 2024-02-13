use std::{fmt::Debug, io, path::PathBuf};

use thiserror::Error;
use tracing::error;

use casper_types::{
    binary_port::RecordId, bytesrepr, crypto, BlockBody, BlockHash, BlockHeader,
    BlockValidationError, DeployHash, Digest, EraId, FinalitySignature, FinalitySignatureId,
    TransactionHash,
};

use super::lmdb_ext::LmdbExtError;
use crate::types::VariantMismatch;

/// A fatal storage component error.
///
/// An error of this kinds indicates that storage is corrupted or otherwise irrecoverably broken, at
/// least for the moment. It should usually be followed by swift termination of the node.
#[derive(Debug, Error)]
pub enum FatalStorageError {
    /// Failure to create the root database directory.
    #[error("failed to create database directory `{}`: {}", .0.display(), .1)]
    CreateDatabaseDirectory(PathBuf, io::Error),
    /// Found a duplicate block-at-height index entry.
    #[error("duplicate entries for block at height {height}: {first} / {second}")]
    DuplicateBlockIndex {
        /// Height at which duplicate was found.
        height: u64,
        /// First block hash encountered at `height`.
        first: BlockHash,
        /// Second block hash encountered at `height`.
        second: BlockHash,
    },
    /// Found a duplicate switch-block-at-era-id index entry.
    #[error("duplicate entries for switch block at era id {era_id}: {first} / {second}")]
    DuplicateEraIdIndex {
        /// Era ID at which duplicate was found.
        era_id: EraId,
        /// First block hash encountered at `era_id`.
        first: BlockHash,
        /// Second block hash encountered at `era_id`.
        second: BlockHash,
    },
    /// Found a duplicate transaction index entry.
    #[error("duplicate entries for blocks for transaction {transaction_hash}: {first} / {second}")]
    DuplicateTransactionIndex {
        /// Transaction hash at which duplicate was found.
        transaction_hash: TransactionHash,
        /// First block hash encountered at `transaction_hash`.
        first: BlockHash,
        /// Second block hash encountered at `transaction_hash`.
        second: BlockHash,
    },
    /// LMDB error while operating.
    #[error("internal database error: {0}")]
    InternalStorage(#[from] LmdbExtError),
    /// An internal DB error - blocks should be overwritten.
    #[error("failed overwriting block")]
    FailedToOverwriteBlock,
    /// Record specified in raw request has not been found in the storage module.
    #[error("unable to find db for record: {0}")]
    DatabaseNotFound(RecordId),
    /// Filesystem error while trying to move file.
    #[error("unable to move file {source_path} to {dest_path}: {original_error}")]
    UnableToMoveFile {
        /// The path to the file that should have been moved.
        source_path: PathBuf,
        /// The path where the file should have been moved to.
        dest_path: PathBuf,
        /// The original `io::Error` from `fs::rename`.
        original_error: io::Error,
    },
    /// Mix of missing and found storage files.
    #[error("expected files to exist: {missing_files:?}.")]
    MissingStorageFiles {
        /// The files that were not be found in the storage directory.
        missing_files: Vec<PathBuf>,
    },
    /// Error when validating a block.
    #[error(transparent)]
    BlockValidation(#[from] BlockValidationError),
    /// A block header was not stored under its hash.
    #[error(
        "Block header not stored under its hash. \
         Queried block hash bytes: {queried_block_hash_bytes:x?}, \
         Found block header hash bytes: {found_block_header_hash:x?}, \
         Block header: {block_header}"
    )]
    BlockHeaderNotStoredUnderItsHash {
        /// The queried block hash.
        queried_block_hash_bytes: Vec<u8>,
        /// The actual header of the block hash.
        found_block_header_hash: BlockHash,
        /// The block header found in storage.
        block_header: Box<BlockHeader>,
    },
    /// Block body did not have a block header.
    #[error(
        "No block header corresponding to block body found in LMDB. \
         Block body hash: {block_body_hash:?}, \
         Block body: {block_body:?}"
    )]
    NoBlockHeaderForBlockBody {
        /// The block body hash.
        block_body_hash: Digest,
        /// The block body.
        block_body: Box<BlockBody>,
    },
    /// Could not verify finality signatures for block.
    #[error("{0} in signature verification. Database is corrupted.")]
    SignatureVerification(crypto::Error),
    /// Corrupted block signature index.
    #[error(
        "Block signatures not indexed by their block hash. \
         Key bytes in LMDB: {raw_key:x?}, \
         Block hash bytes in record: {block_hash_bytes:x?}"
    )]
    CorruptedBlockSignatureIndex {
        /// The key in the block signature index.
        raw_key: Vec<u8>,
        /// The block hash of the signatures found in the index.
        block_hash_bytes: Vec<u8>,
    },
    /// Switch block does not contain era end.
    #[error("switch block does not contain era end: {0:?}")]
    InvalidSwitchBlock(Box<BlockHeader>),
    /// A block body was found to have more parts than expected.
    #[error(
        "Found an unexpected part of a block body in the database: \
        {part_hash:?}"
    )]
    UnexpectedBlockBodyPart {
        /// The block body with the issue.
        block_body_hash: Digest,
        /// The hash of the superfluous body part.
        part_hash: Digest,
    },
    /// Failed to serialize an item that was found in local storage.
    #[error("failed to serialized stored item")]
    StoredItemSerializationFailure(#[source] bincode::Error),
    /// We tried to store finalized approvals for a nonexistent transaction.
    #[error("Tried to store FinalizedApprovals for a nonexistent transaction {transaction_hash}")]
    UnexpectedFinalizedApprovals {
        /// The missing transaction hash.
        transaction_hash: TransactionHash,
    },
    /// `ToBytes` serialization failure of an item that should never fail to serialize.
    #[error("unexpected serialization failure: {0}")]
    UnexpectedSerializationFailure(bytesrepr::Error),
    /// `ToBytes` deserialization failure of an item that should never fail to serialize.
    #[error("unexpected deserialization failure: {0}")]
    UnexpectedDeserializationFailure(bytesrepr::Error),
    /// Stored finalized approvals hashes count doesn't match number of deploys.
    #[error(
        "stored finalized approvals hashes count doesn't match number of deploys: \
        block hash: {block_hash}, expected: {expected}, actual: {actual}"
    )]
    ApprovalsHashesLengthMismatch {
        /// The block hash.
        block_hash: BlockHash,
        /// The number of deploys in the block.
        expected: usize,
        /// The number of approvals hashes.
        actual: usize,
    },
    /// V1 execution results hashmap doesn't have exactly one entry.
    #[error(
        "stored v1 execution results doesn't have exactly one entry: deploy: {deploy_hash}, number \
        of entries: {results_length}"
    )]
    InvalidExecutionResultsV1Length {
        /// The deploy hash.
        deploy_hash: DeployHash,
        /// The number of execution results.
        results_length: usize,
    },
    /// Error initializing metrics.
    #[error("failed to initialize metrics for storage: {0}")]
    Prometheus(#[from] prometheus::Error),
    /// Type mismatch indicating programmer error.
    #[error(transparent)]
    VariantMismatch(#[from] VariantMismatch),
}

// We wholesale wrap lmdb errors and treat them as internal errors here.
impl From<lmdb::Error> for FatalStorageError {
    fn from(err: lmdb::Error) -> Self {
        LmdbExtError::from(err).into()
    }
}

impl From<Box<BlockValidationError>> for FatalStorageError {
    fn from(err: Box<BlockValidationError>) -> Self {
        Self::BlockValidation(*err)
    }
}

/// An error that may occur when handling a get request.
///
/// Wraps a fatal error, callers should check whether the variant is of the fatal or non-fatal kind.
#[derive(Debug, Error)]
pub(super) enum GetRequestError {
    /// A fatal error occurred.
    #[error(transparent)]
    Fatal(#[from] FatalStorageError),
    /// Failed to serialized an item ID on an incoming item request.
    #[error("failed to deserialize incoming item id")]
    MalformedIncomingItemId(#[source] bincode::Error),
    #[error(
        "id information not matching the finality signature: \
        requested id: {requested_id},\
        signature: {finality_signature}"
    )]
    FinalitySignatureIdMismatch {
        // the ID requested
        requested_id: Box<FinalitySignatureId>,
        // the finality signature read from storage
        finality_signature: Box<FinalitySignature>,
    },
}
