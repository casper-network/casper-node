use std::{
    io,
    path::PathBuf,
    sync::{PoisonError, RwLockReadGuard, RwLockWriteGuard},
};

use thiserror::Error;
use tokio::{sync::AcquireError, task::JoinError};
use tracing::error;

use casper_hashing::Digest;
use casper_types::{crypto, EraId};

use super::{lmdb_ext::LmdbExtError, object_pool::ObjectPool, Indices};
use crate::{
    components::consensus::error::FinalitySignatureError,
    types::{
        error::BlockValidationError, BlockBody, BlockHash, BlockHashAndHeight, BlockHeader,
        DeployHash, HashingAlgorithmVersion,
    },
};

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
    /// Found a duplicate switch-block-at-era-id index entry.
    #[error("duplicate entries for blocks for deploy {deploy_hash}: {first} / {second}")]
    DuplicateDeployIndex {
        /// Deploy hash at which duplicate was found.
        deploy_hash: DeployHash,
        /// First block hash encountered at `deploy_hash`.
        first: BlockHashAndHeight,
        /// Second block hash encountered at `deploy_hash`.
        second: BlockHashAndHeight,
    },
    /// LMDB error while operating.
    #[error("internal database error: {0}")]
    InternalStorage(#[from] LmdbExtError),
    /// An internal DB error - blocks should be overwritten.
    #[error("failed overwriting block")]
    FailedToOverwriteBlock,
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
         Hashing algorithm version: {hashing_algorithm_version:?}, \
         Block body: {block_body:?}"
    )]
    NoBlockHeaderForBlockBody {
        /// The block body hash.
        block_body_hash: Digest,
        /// The hashing algorithm of the block body.
        hashing_algorithm_version: HashingAlgorithmVersion,
        /// The block body.
        block_body: Box<BlockBody>,
    },
    /// Unexpected hashing algorithm version.
    #[error(
        "Unexpected hashing algorithm version. \
         Expected: {expected_hashing_algorithm_version:?}, \
         Actual: {actual_hashing_algorithm_version:?}"
    )]
    UnexpectedHashingAlgorithmVersion {
        /// Expected hashing algorithm version.
        expected_hashing_algorithm_version: HashingAlgorithmVersion,
        /// Actual hashing algorithm version.
        actual_hashing_algorithm_version: HashingAlgorithmVersion,
    },
    /// Could not find block body part.
    #[error(
        "Could not find block body part with Merkle linked list node hash: \
         {merkle_linked_list_node_hash:?}"
    )]
    CouldNotFindBlockBodyPart {
        /// The block hash queried.
        block_hash: BlockHash,
        /// The hash of the node in the Merkle linked list.
        merkle_linked_list_node_hash: Digest,
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
    /// Insufficient or wrong finality signatures.
    #[error(transparent)]
    FinalitySignature(#[from] FinalitySignatureError),
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
    /// We tried to store finalized approvals for a nonexistent deploy.
    #[error(
        "Tried to store FinalizedApprovals for a nonexistent deploy. Deploy hash: {deploy_hash:?}"
    )]
    UnexpectedFinalizedApprovals {
        /// The missing deploy hash.
        deploy_hash: DeployHash,
    },
    /// Locking the indices lock failed due to lock poisoning.
    ///
    /// A thread holding the lock has crashed.
    #[error("indices lock poisoned")]
    IndicesLockPoisoned,
    /// Locking the item memory pool lock failed due to lock poisoning.
    ///
    /// A thread holding the lock has crashed.
    #[error("item pool lock poisoned")]
    ItemPoolLockPoisoned,
    /// Failed to join the storage background task.
    #[error("failed to join storage background task")]
    FailedToJoinBackgroundTask(JoinError),
    /// The semaphore guarding parallel sync task execution was closed.
    ///
    /// This should not happen outside of crashes.
    #[error("task semaphore was unexpectedly closed")]
    SemaphoreClosedUnexpectedly(AcquireError),
}

// We wholesale wrap lmdb errors and treat them as internal errors here.
impl From<lmdb::Error> for FatalStorageError {
    fn from(err: lmdb::Error) -> Self {
        LmdbExtError::from(err).into()
    }
}

// While we usually avoid blanked `From` impls on errors, the type is specific enough (includes
// `Indices`) to make an exception to this rule here for both read and write lock poisoning.
impl<'a> From<PoisonError<RwLockReadGuard<'a, Indices>>> for FatalStorageError {
    fn from(_: PoisonError<RwLockReadGuard<'a, Indices>>) -> Self {
        FatalStorageError::IndicesLockPoisoned
    }
}

impl<'a> From<PoisonError<RwLockWriteGuard<'a, Indices>>> for FatalStorageError {
    fn from(_: PoisonError<RwLockWriteGuard<'a, Indices>>) -> Self {
        FatalStorageError::IndicesLockPoisoned
    }
}

impl<'a> From<PoisonError<RwLockReadGuard<'a, ObjectPool<Box<[u8]>>>>> for FatalStorageError {
    fn from(_: PoisonError<RwLockReadGuard<'a, ObjectPool<Box<[u8]>>>>) -> Self {
        FatalStorageError::ItemPoolLockPoisoned
    }
}

impl<'a> From<PoisonError<RwLockWriteGuard<'a, ObjectPool<Box<[u8]>>>>> for FatalStorageError {
    fn from(_: PoisonError<RwLockWriteGuard<'a, ObjectPool<Box<[u8]>>>>) -> Self {
        FatalStorageError::ItemPoolLockPoisoned
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
    /// Received a get request for a gossiped address, which is unanswerable.
    #[error("received a request for a gossiped address")]
    GossipedAddressNotGettable,
}
