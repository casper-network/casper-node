use core::{convert::TryFrom, fmt};

/// The error code indicating the result of handling the binary request.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
#[repr(u8)]
pub enum ErrorCode {
    /// Request executed correctly.
    #[cfg_attr(feature = "std", error("request executed correctly"))]
    NoError = 0,
    /// This function is disabled.
    #[cfg_attr(feature = "std", error("this function is disabled"))]
    FunctionDisabled = 1,
    /// Data not found.
    #[cfg_attr(feature = "std", error("data not found"))]
    NotFound = 2,
    /// Root not found.
    #[cfg_attr(feature = "std", error("root not found"))]
    RootNotFound = 3,
    /// Invalid deploy item variant.
    #[cfg_attr(feature = "std", error("invalid deploy item variant"))]
    InvalidDeployItemVariant = 4,
    /// Wasm preprocessing.
    #[cfg_attr(feature = "std", error("wasm preprocessing"))]
    WasmPreprocessing = 5,
    /// Invalid protocol version.
    #[cfg_attr(feature = "std", error("unsupported protocol version"))]
    UnsupportedProtocolVersion = 6,
    /// Invalid transaction.
    #[cfg_attr(feature = "std", error("invalid transaction"))]
    InvalidTransaction = 7,
    /// Internal error.
    #[cfg_attr(feature = "std", error("internal error"))]
    InternalError = 8,
    /// The query to global state failed.
    #[cfg_attr(feature = "std", error("the query to global state failed"))]
    QueryFailedToExecute = 9,
    /// Bad request.
    #[cfg_attr(feature = "std", error("bad request"))]
    BadRequest = 10,
    /// Received an unsupported type of request.
    #[cfg_attr(feature = "std", error("unsupported request"))]
    UnsupportedRequest = 11,
}

impl TryFrom<u8> for ErrorCode {
    type Error = UnknownErrorCode;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ErrorCode::NoError),
            1 => Ok(ErrorCode::FunctionDisabled),
            2 => Ok(ErrorCode::NotFound),
            3 => Ok(ErrorCode::RootNotFound),
            4 => Ok(ErrorCode::InvalidDeployItemVariant),
            5 => Ok(ErrorCode::WasmPreprocessing),
            6 => Ok(ErrorCode::UnsupportedProtocolVersion),
            7 => Ok(ErrorCode::InvalidTransaction),
            8 => Ok(ErrorCode::InternalError),
            9 => Ok(ErrorCode::QueryFailedToExecute),
            10 => Ok(ErrorCode::BadRequest),
            11 => Ok(ErrorCode::UnsupportedRequest),
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

#[cfg(feature = "std")]
impl std::error::Error for UnknownErrorCode {}
