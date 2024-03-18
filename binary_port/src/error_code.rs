use core::{convert::TryFrom, fmt};

use casper_execution_engine::engine_state::Error as EngineError;
use casper_types::InvalidTransaction;

/// The error code indicating the result of handling the binary request.
#[derive(Debug, Clone, thiserror::Error)]
#[repr(u8)]
pub enum ErrorCode {
    /// Request executed correctly.
    #[error("request executed correctly")]
    NoError = 0,
    /// This function is disabled.
    #[error("this function is disabled")]
    FunctionDisabled = 1,
    /// Data not found.
    #[error("data not found")]
    NotFound = 2,
    /// Root not found.
    #[error("root not found")]
    RootNotFound = 3,
    /// Invalid deploy item variant.
    #[error("invalid deploy item variant")]
    InvalidItemVariant = 4,
    /// Wasm preprocessing.
    #[error("wasm preprocessing")]
    WasmPreprocessing = 5,
    /// Invalid protocol version.
    #[error("unsupported protocol version")]
    UnsupportedProtocolVersion = 6,
    /// Invalid transaction.
    #[error("invalid transaction")]
    InvalidTransaction = 7,
    /// Internal error.
    #[error("internal error")]
    InternalError = 8,
    /// The query to global state failed.
    #[error("the query to global state failed")]
    FailedQuery = 9,
    /// Bad request.
    #[error("bad request")]
    BadRequest = 10,
    /// Received an unsupported type of request.
    #[error("unsupported request")]
    UnsupportedRequest = 11,
    /// This node has no complete blocks.
    #[error("no complete blocks")]
    NoCompleteBlocks = 12,
}

impl TryFrom<u8> for ErrorCode {
    type Error = UnknownErrorCode;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ErrorCode::NoError),
            1 => Ok(ErrorCode::FunctionDisabled),
            2 => Ok(ErrorCode::NotFound),
            3 => Ok(ErrorCode::RootNotFound),
            4 => Ok(ErrorCode::InvalidItemVariant),
            5 => Ok(ErrorCode::WasmPreprocessing),
            6 => Ok(ErrorCode::UnsupportedProtocolVersion),
            7 => Ok(ErrorCode::InvalidTransaction),
            8 => Ok(ErrorCode::InternalError),
            9 => Ok(ErrorCode::FailedQuery),
            10 => Ok(ErrorCode::BadRequest),
            11 => Ok(ErrorCode::UnsupportedRequest),
            12 => Ok(ErrorCode::NoCompleteBlocks),
            _ => Err(UnknownErrorCode),
        }
    }
}

/// Error indicating that the error code is unknown.
#[derive(Debug, Clone, Copy)]
pub struct UnknownErrorCode;

impl fmt::Display for UnknownErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown node error code")
    }
}

impl std::error::Error for UnknownErrorCode {}

impl From<EngineError> for ErrorCode {
    fn from(err: EngineError) -> Self {
        match err {
            EngineError::RootNotFound(_) => ErrorCode::RootNotFound,
            EngineError::Deploy => ErrorCode::InvalidTransaction,
            EngineError::InvalidItemVariant(_) => ErrorCode::InvalidItemVariant,
            EngineError::WasmPreprocessing(_) => ErrorCode::WasmPreprocessing,
            EngineError::InvalidProtocolVersion(_) => ErrorCode::UnsupportedProtocolVersion,
            EngineError::Deprecated(_) => ErrorCode::FunctionDisabled,
            _ => ErrorCode::InternalError,
        }
    }
}

impl From<InvalidTransaction> for ErrorCode {
    fn from(_value: InvalidTransaction) -> Self {
        ErrorCode::InvalidTransaction
    }
}
