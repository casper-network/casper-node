//! Errors that the contract runtime component may raise.
use derive_more::From;
use std::collections::BTreeMap;

use serde::Serialize;
use thiserror::Error;

use casper_execution_engine::engine_state::{Error as EngineStateError, StepError};
use casper_storage::{
    global_state::error::Error as GlobalStateError, tracking_copy::TrackingCopyError,
};
use casper_types::{bytesrepr, CLValueError, PublicKey, U512};

use crate::{
    components::contract_runtime::ExecutionPreState,
    types::{ChunkingError, ExecutableBlock, InternalEraReport},
};

/// An error returned from mis-configuring the contract runtime component.
#[derive(Debug, Error)]
pub(crate) enum ConfigError {
    /// Error initializing the LMDB environment.
    #[error("failed to initialize LMDB environment for contract runtime: {0}")]
    GlobalState(#[from] GlobalStateError),
    /// Error initializing metrics.
    #[error("failed to initialize metrics for contract runtime: {0}")]
    Prometheus(#[from] prometheus::Error),
}

/// An enum that represents all possible error conditions of a `contract_runtime` component.
#[derive(Debug, Error, From)]
pub(crate) enum ContractRuntimeError {
    /// The provided serialized id cannot be deserialized properly.
    #[error("error deserializing id: {0}")]
    InvalidSerializedId(#[source] bincode::Error),
    // It was not possible to get trie with the specified id
    #[error("error retrieving trie by id: {0}")]
    FailedToRetrieveTrieById(#[source] GlobalStateError),
    /// Chunking error.
    #[error("failed to chunk the data {0}")]
    ChunkingError(#[source] ChunkingError),
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
         block: {executable_block:?}, \
         execution pre-state: {execution_pre_state:?}"
    )]
    WrongBlockHeight {
        /// The finalized block the system attempted to execute.
        executable_block: Box<ExecutableBlock>,
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
    /// Failed to compute the approvals checksum.
    #[error("failed to compute approvals checksum: {0}")]
    FailedToComputeApprovalsChecksum(bytesrepr::Error),
    /// Failed to compute the execution results checksum.
    #[error("failed to compute execution results checksum: {0}")]
    FailedToComputeExecutionResultsChecksum(bytesrepr::Error),
    /// Failed to convert the checksum registry to a `CLValue`.
    #[error("failed to convert the checksum registry to a clvalue: {0}")]
    ChecksumRegistryToCLValue(CLValueError),
    /// `EraEnd`s need both an `EraReport` present and a map of the next era validator weights.
    /// If one of them is not present while trying to construct an `EraEnd`, this error is
    /// produced.
    #[error(
        "cannot create era end unless we have both an era report and next era validators. \
         era report: {maybe_era_report:?}, \
         next era validator weights: {maybe_next_era_validator_weights:?}"
    )]
    FailedToCreateEraEnd {
        /// An optional `EraReport` we tried to use to construct an `EraEnd`.
        maybe_era_report: Option<InternalEraReport>,
        /// An optional map of the next era validator weights used to construct an `EraEnd`.
        maybe_next_era_validator_weights: Option<BTreeMap<PublicKey, U512>>,
    },
    /// An error that occurred while interacting with lmdb.
    #[error(transparent)]
    Lmdb(
        #[from]
        #[serde(skip_serializing)]
        GlobalStateError,
    ),
    /// An error that occurred while getting era validators.
    #[error(transparent)]
    GetEraValidators(
        #[from]
        #[serde(skip_serializing)]
        TrackingCopyError,
    ),
}
