use thiserror::Error;

use casper_execution_engine::core::engine_state;
use casper_types::{bytesrepr, crypto::ErrorExt as CryptoError};

use crate::{
    components::{
        contract_runtime, contract_runtime::BlockExecutionError, diagnostics_port, network,
        storage, upgrade_watcher,
    },
    utils::{ListeningError, LoadError},
};

/// Error type returned by the validator reactor.
#[derive(Debug, Error)]
pub(crate) enum Error {
    /// `UpgradeWatcher` component error.
    #[error("upgrade watcher error: {0}")]
    UpgradeWatcher(#[from] upgrade_watcher::Error),

    /// Metrics-related error
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),

    /// `Network` component error.
    #[error("network error: {0}")]
    Network(#[from] network::Error),

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
