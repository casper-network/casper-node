use casper_execution_engine::engine_state;
use casper_types::{binary_port::db_id::DbId, bytesrepr};
use thiserror::Error;

use crate::components::transaction_acceptor;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("Serialization error: {}", _0)]
    BytesRepr(bytesrepr::Error),
    #[error("Execution engine error: {}", _0)]
    EngineState(engine_state::Error),
    #[error("Transaction acceptor: {}", _0)]
    TransactionAcceptor(transaction_acceptor::Error),
    #[error("This function is disabled: {}", _0)]
    FunctionDisabled(String),
    #[error("No such database: {}", _0)]
    NoSuchDatabase(DbId),
}

#[repr(u8)]
pub enum ErrorCode {
    NoError = 0,
    Serialization = 1,
    InvalidTransaction = 2,
    FunctionDisabled = 3,
    InternalError = 4,
    NoSuchDatabase = 5,
}

impl Error {
    fn as_error_code(&self) -> u8 {
        match self {
            Error::BytesRepr(_) => ErrorCode::Serialization as u8,
            Error::EngineState(_) => ErrorCode::InternalError as u8,
            Error::TransactionAcceptor(_) => ErrorCode::InvalidTransaction as u8,
            Error::FunctionDisabled(_) => ErrorCode::FunctionDisabled as u8,
            Error::NoSuchDatabase(_) => ErrorCode::NoSuchDatabase as u8,
        }
    }
}
