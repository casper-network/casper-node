use casper_execution_engine::engine_state;
use casper_types::bytesrepr;
use thiserror::Error;

use crate::components::transaction_acceptor;

#[derive(Debug, Error)]
pub(crate) enum SpeculativeExecutionError {
    // EngineStateError::RootNotFound(_) => Error::new(ErrorCode::NoSuchStateRoot, ""),
    #[error("No such state root")]
    NoSuchStateRoot,

    // EngineStateError::InvalidDeployItemVariant(error)
    // EngineStateError::WasmPreprocessing(error) => { Error::new(ErrorCode::InvalidDeploy, error.to_string())}
    // EngineStateError::InvalidProtocolVersion(_) => Error::new(ErrorCode::InvalidDeploy, format!("deploy used invalid protocol version {}", error),),
    // EngineStateError::Deploy
    #[error("Invalid deploy: {}", _0)]
    InvalidDeploy(String),

    // Error::new(ReservedErrorCode::InternalError, error.to_string())
    #[error("Internal error: {}", _0)]
    InternalError(String),
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("Serialization error: {}", _0)]
    BytesRepr(bytesrepr::Error),
    #[error("Execution engine error: {}", _0)]
    EngineState(engine_state::Error),
    #[error("Speculative execution: {}", _0)]
    SpeculativeExecution(SpeculativeExecutionError),
    #[error("Transaction acceptor: {}", _0)]
    TransactionAcceptor(transaction_acceptor::Error),
}
