//! Errors that the contract runtime component may raise.

use serde::Serialize;
use thiserror::Error;

use casper_execution_engine::{
    core::engine_state::{Error as EngineStateError, StepError},
    storage::error::lmdb::Error as StorageLmdbError,
};

use crate::{
    components::contract_runtime::ExecutionPreState,
    types::{error::BlockCreationError, FinalizedBlock},
};
use casper_execution_engine::core::engine_state::GetEraValidatorsError;

/// An error returned from mis-configuring the contract runtime component.
#[derive(Debug, Error)]
pub(crate) enum ConfigError {
    /// Error initializing the LMDB environment.
    #[error("failed to initialize LMDB environment for contract runtime: {0}")]
    Lmdb(#[from] StorageLmdbError),
    /// Error initializing metrics.
    #[error("failed to initialize metrics for contract runtime: {0}")]
    Prometheus(#[from] prometheus::Error),
    /// Error initializing execution engine.
    #[error("failed to initialize execution engine: {0}")]
    EngineState(#[from] EngineStateError),
}

/// An error during block execution.
#[derive(Debug, Error, Serialize)]
pub enum BlockExecutionError {
    /// Currently the contract runtime can only execute one commit at a time, so we cannot handle
    /// more than one execution result.
    #[error("more than one execution result")]
    MoreThanOneExecutionResult,
    /// Both the block to be executed and the execution pre-state specify the height of the next
    /// block. These must agree and this error will be thrown if they do not.
    #[error(
        "block's height does not agree with execution pre-state. \
         block: {finalized_block:?}, \
         execution pre-state: {execution_pre_state:?}"
    )]
    WrongBlockHeight {
        /// The finalized block the system attempted to execute.
        finalized_block: Box<FinalizedBlock>,
        /// The state of the block chain prior to block execution that was to be used.
        execution_pre_state: Box<ExecutionPreState>,
    },
    /// A core error thrown by the execution engine.
    #[error(transparent)]
    EngineState(
        #[from]
        #[serde(skip_serializing)]
        EngineStateError,
    ),
    /// An error that occurred when trying to run the auction contract.
    #[error(transparent)]
    Step(
        #[from]
        #[serde(skip_serializing)]
        StepError,
    ),
    /// An error that occurred while creating a block.
    #[error(transparent)]
    BlockCreation(#[from] BlockCreationError),
    /// An error that occurred while interacting with lmdb.
    #[error(transparent)]
    Lmdb(
        #[from]
        #[serde(skip_serializing)]
        lmdb::Error,
    ),
    /// An error that occurred while getting era validators.
    #[error(transparent)]
    GetEraValidators(
        #[from]
        #[serde(skip_serializing)]
        GetEraValidatorsError,
    ),
}
