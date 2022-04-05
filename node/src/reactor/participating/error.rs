use thiserror::Error;

use crate::{
    components::{
        chain_synchronizer, contract_runtime, contract_runtime::BlockExecutionError,
        diagnostics_port, small_network, storage,
    },
    crypto::Error as CryptoError,
    utils::{ListeningError, LoadError},
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

    /// `Chain synchronizer` component error.
    #[error("chain synchronizer error: {0}")]
    ChainSynchronizer(#[from] chain_synchronizer::Error),

    /// `DiagnosticsPort` component error.
    #[error("diagnostics port: {0}")]
    DiagnosticsPort(#[from] diagnostics_port::Error),

    /// Error while loading the signing key pair.
    #[error("signing key pair load error: {0}")]
    LoadSigningKeyPair(#[from] LoadError<CryptoError>),
}

impl From<bytesrepr::Error> for Error {
    fn from(err: bytesrepr::Error) -> Self {
        Self::BytesRepr(err)
    }
}
