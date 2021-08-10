//! Errors that the contract runtime component may raise.

use casper_execution_engine::{
    core::engine_state::{Error as EngineStateError, StepError},
    storage::error::lmdb::Error as StorageLmdbError,
};

use crate::{
    components::contract_runtime::ExecutionPreState,
    types::{error::BlockCreationError, FinalizedBlock},
};
use casper_execution_engine::core::engine_state::GetEraValidatorsError;

/// Error returned from mis-configuring the contract runtime component.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ConfigError {
    /// Error initializing the LMDB environment.
    #[error("failed to initialize LMDB environment for contract runtime: {0}")]
    Lmdb(#[from] StorageLmdbError),
    /// Error initializing metrics.
    #[error("failed to initialize metrics for contract runtime: {0}")]
    Prometheus(#[from] prometheus::Error),
}

/// An error raised by a contract runtime variant.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BlockExecutionError {
    /// Currently the contract runtime can only execute one commit at a time, so we cannot handle
    /// more than one execution result.
    #[error("More than one execution result")]
    MoreThanOneExecutionResult,
    /// Both the block to be executed and the execution pre-state specify the height of the next
    /// block. These must agree and this error will be thrown if they do not.
    #[error(
        "Block's height does not agree with execution pre-state. \
         Block: {finalized_block:?}, \
         Execution pre-state: {execution_pre_state:?}"
    )]
    WrongBlockHeight {
        /// The finalized block the system attempted to execute.
        finalized_block: Box<FinalizedBlock>,
        /// The state of the block chain prior to block execution that was to be used.
        execution_pre_state: Box<ExecutionPreState>,
    },
    /// A core error thrown by the execution engine.
    #[error(transparent)]
    EngineStateError(#[from] EngineStateError),
    /// An error that occurred when trying to run the auction contract.
    #[error(transparent)]
    StepError(#[from] StepError),
    /// An error that occurred when committing execution results to the trie.

    /// An error that occurred while creating a block.
    #[error(transparent)]
    BlockCreationError(#[from] BlockCreationError),
}

/// An error raised when block execution events are being created for the reactor.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BlockExecutionEventsError {
    #[error(transparent)]
    BlockExecutionError(#[from] BlockExecutionError),
    #[error(transparent)]
    GetEraValidatorsError(#[from] GetEraValidatorsError),
}
