use std::fmt::Debug;

use serde::Serialize;
use thiserror::Error;
use tokio::{sync::AcquireError, task::JoinError};

use casper_execution_engine::core::{engine_state, engine_state::GetEraValidatorsError};
use casper_hashing::Digest;
use casper_types::{bytesrepr, CLValueError, EraId, ProtocolVersion};

use crate::{
    components::{
        contract_runtime::BlockExecutionError, fetcher::FetcherError, linear_chain,
        linear_chain::BlockSignatureError,
    },
    types::{
        Block, BlockAndDeploys, BlockEffectsOrChunk, BlockHash, BlockHeader,
        BlockHeaderWithMetadata, BlockHeadersBatch, BlockWithMetadata, Deploy,
        FinalizedApprovalsWithId, TrieOrChunk,
    },
};

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
    BlockFetcher(#[from] FetcherError<Block>),

    #[error("no such block hash: {bogus_block_hash}")]
    NoSuchBlockHash { bogus_block_hash: BlockHash },

    #[error("no such block height: {0} encountered during syncing to Genesis")]
    NoSuchBlockHeight(u64),

    #[error("no highest block header")]
    NoHighestBlockHeader,

    #[error(transparent)]
    BlockHeaderFetcher(#[from] FetcherError<BlockHeader>),

    #[error(transparent)]
    BlockHeaderWithMetadataFetcher(#[from] FetcherError<BlockHeaderWithMetadata>),

    #[error(transparent)]
    BlockWithMetadataFetcher(#[from] FetcherError<BlockWithMetadata>),

    #[error(transparent)]
    BlockAndDeploysFetcher(#[from] FetcherError<BlockAndDeploys>),

    #[error(transparent)]
    DeployWithMetadataFetcher(#[from] FetcherError<Deploy>),

    #[error(transparent)]
    FinalizedApprovalsFetcher(#[from] FetcherError<FinalizedApprovalsWithId>),

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

    /// Error fetching block effects.
    #[error(transparent)]
    FetchBlockEffects(
        #[from]
        #[serde(skip_serializing)]
        FetchBlockEffectsError,
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

    /// Error querying global state
    #[error(transparent)]
    GlobalStateQueryError(
        #[from]
        #[serde(skip_serializing)]
        GlobalStateQueryError,
    ),
}

#[derive(Error, Debug)]
pub(crate) enum FetchTrieError {
    /// Fetcher error.
    #[error(transparent)]
    FetcherError(#[from] FetcherError<TrieOrChunk>),

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
}

#[derive(Error, Debug)]
pub(crate) enum FetchBlockHeadersBatchError {
    /// Fetcher error
    #[error(transparent)]
    FetchError(#[from] FetcherError<BlockHeadersBatch>),

    #[error("Batch from storage was empty")]
    EmptyBatchFromStorage,
}

#[derive(Error, Debug)]
pub(crate) enum FetchBlockEffectsError {
    /// Fetcher error.
    #[error(transparent)]
    FetcherError(#[from] FetcherError<BlockEffectsOrChunk>),

    /// BlockEffects were being fetched from peers by chunks but was somehow fetch from storage.
    #[error(
        "Element was being fetched from peers by chunks but was somehow fetched from storage. \
         Perhaps there are parallel downloads going on?"
    )]
    BlockEffectsFetchByChunksSomehowFetchedFromStorage,

    /// BlockEffects was being fetched from peers by chunks but it was retrieved whole by a peer
    /// somehow.
    #[error(
        "BlockEffects was being fetched from peers by chunks but it was retrieved whole \
         by a peer somehow. Block hash: {block_hash:?}"
    )]
    BlockEffectsBeingFetchedByChunksSomehowFetchWholeFromPeer { block_hash: BlockHash },

    /// Deserialization error.
    #[error("Failed to deserialize chunks into Vec<ExecutionResult>: {0}")]
    DeserializationError(String),

    #[error("Missing block in storage when syncing block effects: {0}")]
    MissingBlock(BlockHash),
}

impl From<bytesrepr::Error> for FetchBlockEffectsError {
    fn from(err: bytesrepr::Error) -> Self {
        Self::DeserializationError(format!("{}", err))
    }
}

#[derive(Error, Debug)]
pub(crate) enum GlobalStateQueryError {
    /// Circular reference.
    #[error("Unexpected circular reference: {0}")]
    CircularReference(String),

    /// Query depth limit reached.
    #[error("Query depth limit reached.")]
    DepthLimit,

    /// Error deserializing CL value.
    #[error("Failed to deserialize CL value into expected type: {0}")]
    CLValueError(String),
}

impl GlobalStateQueryError {
    pub(crate) fn invalid_type(expected: String, got: String) -> Self {
        Self::CLValueError(format!(
            "Unexpected type under key. Expected: {}, got: {}",
            expected, got
        ))
    }
}

impl From<CLValueError> for GlobalStateQueryError {
    fn from(err: CLValueError) -> Self {
        Self::CLValueError(format!("{}", err))
    }
}
