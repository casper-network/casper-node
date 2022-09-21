use std::fmt::Debug;

use serde::Serialize;
use thiserror::Error;
use tokio::{sync::AcquireError, task::JoinError};

use casper_execution_engine::core::{engine_state, engine_state::GetEraValidatorsError};
use casper_hashing::Digest;
use casper_types::{EraId, ProtocolVersion};

use crate::{
    components::{
        contract_runtime::BlockExecutionError, fetcher, linear_chain,
        linear_chain::BlockSignatureError,
    },
    types::{
        Block, BlockAndDeploys, BlockDeployApprovals, BlockHash, BlockHeader,
        BlockHeaderWithMetadata, BlockHeadersBatch, BlockWithMetadata, Deploy, FetcherItem,
        TrieOrChunk,
    },
};

use super::operations::FetchWithRetryError;

#[derive(Error, Debug, Serialize)]
pub(crate) enum Error {
    #[error(transparent)]
    ExecutionEngine(
        #[from]
        #[serde(skip_serializing)]
        engine_state::Error,
    ),

    #[error(transparent)]
    LinearChain(#[from] linear_chain::Error),

    #[error(
        "trusted header is from before the last upgrade and isn't the last header before \
         activation. \
         trusted header: {trusted_header:?}, \
         current protocol version: {current_protocol_version:?}, \
         current version activation point: {activation_point:?}"
    )]
    TrustedHeaderTooEarly {
        trusted_header: Box<BlockHeader>,
        current_protocol_version: ProtocolVersion,
        activation_point: EraId,
    },

    #[error("no blocks have been found in storage (should provide recent trusted hash)")]
    NoBlocksInStorage,

    #[error(
        "configured trusted block is different from the stored block at the same height \
         configured block header: {config_header:?}, \
         stored block header: {stored_header_at_same_height:?}"
    )]
    TrustedHeaderOnDifferentFork {
        config_header: Box<BlockHeader>,
        stored_header_at_same_height: Box<BlockHeader>,
    },

    #[error(
        "current version is {current_version}, but retrieved block header with future version: \
         {block_header_with_future_version:?}"
    )]
    RetrievedBlockHeaderFromFutureVersion {
        current_version: ProtocolVersion,
        block_header_with_future_version: Box<BlockHeader>,
    },

    #[error(transparent)]
    BlockFetcher(#[from] fetcher::Error<Block>),

    #[error("no such block hash: {bogus_block_hash}")]
    NoSuchBlockHash { bogus_block_hash: BlockHash },

    #[error("no such block height: {0} encountered during syncing to Genesis")]
    NoSuchBlockHeight(u64),

    #[error("no highest block header")]
    NoHighestBlockHeader,

    #[error(transparent)]
    BlockHeaderFetcher(#[from] fetcher::Error<BlockHeader>),

    #[error(transparent)]
    BlockHeaderWithMetadataFetcher(#[from] fetcher::Error<BlockHeaderWithMetadata>),

    #[error(transparent)]
    BlockWithMetadataFetcher(#[from] fetcher::Error<BlockWithMetadata>),

    #[error(transparent)]
    BlockAndDeploysFetcher(#[from] fetcher::Error<BlockAndDeploys>),

    #[error(transparent)]
    DeployWithMetadataFetcher(#[from] fetcher::Error<Deploy>),

    #[error(transparent)]
    BlockDeployApprovalsFetcher(#[from] fetcher::Error<BlockDeployApprovals>),

    #[error(transparent)]
    FinalitySignatures(
        #[from]
        #[serde(skip_serializing)]
        BlockSignatureError,
    ),

    #[error(transparent)]
    BlockExecution(#[from] BlockExecutionError),

    #[error("hit genesis block trying to get trusted era validators")]
    HitGenesisBlockTryingToGetTrustedEraValidators { trusted_header: BlockHeader },

    /// Error getting era validators from the execution engine.
    #[error(transparent)]
    GetEraValidators(
        #[from]
        #[serde(skip_serializing)]
        GetEraValidatorsError,
    ),

    #[error("stored block has unexpected parent hash. parent: {parent:?}, child: {child:?}")]
    UnexpectedParentHash {
        parent: Box<BlockHeader>,
        child: Box<BlockHeader>,
    },

    #[error("block has a lower version than its parent")]
    LowerVersionThanParent {
        parent: Box<BlockHeader>,
        child: Box<BlockHeader>,
    },

    #[error("parent block has a height of u64::MAX")]
    HeightOverflow { parent: Box<BlockHeader> },

    /// Error joining tokio task.
    #[error(transparent)]
    Join(
        #[from]
        #[serde(skip_serializing)]
        JoinError,
    ),

    /// Metrics-related error
    #[error("prometheus (metrics) error: {0}")]
    Metrics(
        #[from]
        #[serde(skip_serializing)]
        prometheus::Error,
    ),

    /// Error fetching a trie.
    #[error(transparent)]
    FetchTrie(
        #[from]
        #[serde(skip_serializing)]
        FetchTrieError,
    ),

    /// Error fetching block headers batch.
    #[error(transparent)]
    FetchHeadersBatch(
        #[from]
        #[serde(skip_serializing)]
        FetchBlockHeadersBatchError,
    ),

    /// Semaphore closed unexpectedly.
    #[error(transparent)]
    SemaphoreError(
        #[from]
        #[serde(skip_serializing)]
        AcquireError,
    ),

    #[error("fetch attempts exhausted")]
    AttemptsExhausted,
}

#[derive(Error, Debug)]
pub(crate) enum FetchTrieError {
    /// Fetcher error.
    #[error(transparent)]
    FetcherError(#[from] fetcher::Error<TrieOrChunk>),

    /// Trie was being fetched from peers by chunks but was somehow fetch from storage.
    #[error(
        "Trie was being fetched from peers by chunks but was somehow fetched from storage. \
         Perhaps there are parallel downloads going on?"
    )]
    TrieBeingFetchByChunksSomehowFetchedFromStorage,

    /// Trie was being fetched from peers by chunks but it was retrieved whole by a peer somehow.
    #[error(
        "Trie was being fetched from peers by chunks but it was retrieved whole \
         by a peer somehow. Trie digest: {digest:?}"
    )]
    TrieBeingFetchedByChunksSomehowFetchWholeFromPeer { digest: Digest },

    #[error("fetch attempts exhausted")]
    AttemptsExhausted,
}

impl<T> From<FetchWithRetryError<T>> for FetchTrieError
where
    FetchTrieError: From<fetcher::Error<T>>,
    T: FetcherItem,
{
    fn from(err: FetchWithRetryError<T>) -> Self {
        match err {
            FetchWithRetryError::AttemptsExhausted { .. } => FetchTrieError::AttemptsExhausted,
            FetchWithRetryError::FetcherError(err) => err.into(),
        }
    }
}

impl<T> From<FetchWithRetryError<T>> for FetchBlockHeadersBatchError
where
    FetchBlockHeadersBatchError: From<fetcher::Error<T>>,
    T: FetcherItem,
{
    fn from(err: FetchWithRetryError<T>) -> Self {
        match err {
            FetchWithRetryError::AttemptsExhausted { .. } => {
                FetchBlockHeadersBatchError::AttemptsExhausted
            }
            FetchWithRetryError::FetcherError(err) => err.into(),
        }
    }
}

impl<T> From<FetchWithRetryError<T>> for Error
where
    Error: From<fetcher::Error<T>>,
    T: FetcherItem,
{
    fn from(err: FetchWithRetryError<T>) -> Self {
        match err {
            FetchWithRetryError::AttemptsExhausted { .. } => Error::AttemptsExhausted,
            FetchWithRetryError::FetcherError(err) => err.into(),
        }
    }
}

#[derive(Error, Debug)]
pub(crate) enum FetchBlockHeadersBatchError {
    /// Fetcher error
    #[error(transparent)]
    FetchError(#[from] fetcher::Error<BlockHeadersBatch>),

    #[error("Batch from storage was empty")]
    EmptyBatchFromStorage,

    #[error("fetch attempts exhausted")]
    AttemptsExhausted,
}
