use thiserror::Error;

use crate::{
    components::{
        console, contract_runtime, contract_runtime::BlockExecutionError, small_network, storage,
    },
    utils::ListeningError,
};
use casper_execution_engine::core::engine_state;
use casper_types::bytesrepr;

/// Error type returned by the validator reactor.
#[derive(Debug, Error)]
pub(crate) enum Error {
    /// Metrics-related error
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),

    /// `SmallNetwork` component error.
    #[error("small network error: {0}")]
    SmallNetwork(#[from] small_network::Error),

    /// An error starting one of the HTTP servers.
    #[error("http server listening error: {0}")]
    HttpServerListening(#[from] ListeningError),

    /// `Storage` component error.
    #[error("storage error: {0}")]
    Storage(#[from] storage::FatalStorageError),

    /// `Consensus` component error.
    #[error("consensus error: {0}")]
    Consensus(#[from] anyhow::Error),

    /// `ContractRuntime` component error.
    #[error("contract runtime config error: {0}")]
    ContractRuntime(#[from] contract_runtime::ConfigError),

    /// Block execution error.
    #[error(transparent)]
    BlockExecution(#[from] BlockExecutionError),

    /// Engine state error.
    #[error(transparent)]
    EngineState(#[from] engine_state::Error),

    /// [`bytesrepr`] error.
    #[error("bytesrepr error: {0}")]
    BytesRepr(bytesrepr::Error),

    /// Joining outcome invalid or missing after running chain sync.
    #[error("joining outcome invalid or missing after running chain synchronization")]
    InvalidJoiningOutcome,

    /// `Console` component error.
    #[error("console error: {0}")]
    Console(#[from] console::Error),
}

impl From<bytesrepr::Error> for Error {
    fn from(err: bytesrepr::Error) -> Self {
        Self::BytesRepr(err)
    }
}
