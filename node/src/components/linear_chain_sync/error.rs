use std::fmt::Debug;

use thiserror::Error;
use tokio::task::JoinError;

use casper_execution_engine::{
    core::{engine_state, engine_state::GetEraValidatorsError},
    storage::trie::Trie,
};
use casper_types::{EraId, Key, ProtocolVersion, StoredValue};

use crate::{
    components::{contract_runtime::BlockExecutionError, fetcher::FetcherError},
    types::{
        Block, BlockHash, BlockHeader, BlockHeaderWithMetadata, BlockWithMetadata, Deploy, NodeId,
    },
};

#[derive(Error, Debug)]
pub(crate) enum LinearChainSyncError {
    #[error(transparent)]
    ExecutionEngineError(#[from] engine_state::Error),

    #[error(
        "Cannot get trusted validators for such an early era. \
         trusted header: {trusted_header:?}, \
         last emergency restart era id: {maybe_last_emergency_restart_era_id:?}"
    )]
    TrustedHeaderEraTooEarly {
        trusted_header: Box<BlockHeader>,
        maybe_last_emergency_restart_era_id: Option<EraId>,
    },

    #[error(
        "Current version is {current_version}, but retrieved block header with future version: \
         {block_header_with_future_version:?}"
    )]
    RetrievedBlockHeaderFromFutureVersion {
        current_version: ProtocolVersion,
        block_header_with_future_version: Box<BlockHeader>,
    },

    #[error(
        "Current version is {current_version}, but current block header has older version: \
         {block_header_with_old_version:?}"
    )]
    CurrentBlockHeaderHasOldVersion {
        current_version: ProtocolVersion,
        block_header_with_old_version: Box<BlockHeader>,
    },

    #[error(transparent)]
    BlockFetcherError(#[from] FetcherError<Block, NodeId>),

    #[error("No such block hash: {bogus_block_hash}")]
    NoSuchBlockHash { bogus_block_hash: BlockHash },

    #[error(transparent)]
    BlockHeaderFetcherError(#[from] FetcherError<BlockHeader, NodeId>),

    #[error(transparent)]
    BlockHeaderWithMetadataFetcherError(#[from] FetcherError<BlockHeaderWithMetadata, NodeId>),

    #[error(transparent)]
    BlockWithMetadataFetcherError(#[from] FetcherError<BlockWithMetadata, NodeId>),

    #[error(transparent)]
    DeployWithMetadataFetcherError(#[from] FetcherError<Deploy, NodeId>),

    #[error(transparent)]
    TrieFetcherError(#[from] FetcherError<Trie<Key, StoredValue>, NodeId>),

    #[error(
        "Executed block is not the same as downloaded block. \
         Executed block: {executed_block:?}, \
         Downloaded block: {downloaded_block:?}"
    )]
    ExecutedBlockIsNotTheSameAsDownloadedBlock {
        executed_block: Box<Block>,
        downloaded_block: Box<Block>,
    },

    #[error(transparent)]
    BlockExecutionError(#[from] BlockExecutionError),

    #[error(
        "Joining with trusted hash before emergency restart not supported. \
         Find a more recent hash from after the restart. \
         Last emergency restart era: {last_emergency_restart_era}, \
         Trusted hash: {trusted_hash:?}, \
         Trusted block header: {trusted_block_header:?}"
    )]
    TryingToJoinBeforeLastEmergencyRestartEra {
        last_emergency_restart_era: EraId,
        trusted_hash: BlockHash,
        trusted_block_header: Box<BlockHeader>,
    },

    #[error("Hit genesis block trying to get trusted era validators.")]
    HitGenesisBlockTryingToGetTrustedEraValidators { trusted_header: BlockHeader },

    /// Error getting era validators from the execution engine.
    #[error(transparent)]
    GetEraValidatorsError(#[from] GetEraValidatorsError),

    #[error("Stored block has unexpected parent hash. parent: {parent:?}, child: {child:?}")]
    UnexpectedParentHash {
        parent: Box<BlockHeader>,
        child: Box<BlockHeader>,
    },

    #[error("Block has a lower version than its parent.")]
    LowerVersionThanParent {
        parent: Box<BlockHeader>,
        child: Box<BlockHeader>,
    },

    #[error("Parent block has a height of u64::MAX.")]
    HeightOverflow { parent: Box<BlockHeader> },

    /// Error joining tokio task.
    #[error(transparent)]
    Join(#[from] JoinError),
}
