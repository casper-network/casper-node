use std::fmt::Debug;

use serde::Serialize;
use thiserror::Error;
use tokio::task::JoinError;

use casper_execution_engine::{
    core::{engine_state, engine_state::GetEraValidatorsError},
    storage::trie::TrieOrChunk,
};
use casper_hashing::Digest;
use casper_types::{EraId, ProtocolVersion};

use crate::{
    components::{contract_runtime::BlockExecutionError, fetcher::FetcherError},
    types::{
        Block, BlockAndDeploys, BlockHash, BlockHeader, BlockHeaderWithMetadata, BlockWithMetadata,
        Deploy, FinalizedApprovalsWithId,
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

    #[error(
        "cannot get trusted validators for such an early era. \
         trusted header: {trusted_header:?}, \
         last emergency restart era id: {maybe_last_emergency_restart_era_id:?}"
    )]
    TrustedHeaderEraTooEarly {
        trusted_header: Box<BlockHeader>,
        maybe_last_emergency_restart_era_id: Option<EraId>,
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
    BlockExecution(#[from] BlockExecutionError),

    #[error(
        "joining with trusted hash before emergency restart not supported - find a more recent \
         hash from after the restart. \
         last emergency restart era: {last_emergency_restart_era}, \
         trusted hash: {trusted_hash:?}, \
         trusted block header: {trusted_block_header:?}"
    )]
    TryingToJoinBeforeLastEmergencyRestartEra {
        last_emergency_restart_era: EraId,
        trusted_hash: BlockHash,
        trusted_block_header: Box<BlockHeader>,
    },

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
