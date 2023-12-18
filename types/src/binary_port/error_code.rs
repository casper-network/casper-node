//! Binary port error.

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
    FunctionIsDisabled = 1,
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
    #[cfg_attr(feature = "std", error("invalid protocol version"))]
    InvalidProtocolVersion = 6,
    /// Invalid deploy.
    #[cfg_attr(feature = "std", error("invalid deploy"))]
    InvalidDeploy = 7,
    /// Internal error.
    #[cfg_attr(feature = "std", error("internal error"))]
    InternalError = 8,
    /// The query to global state failed.
    #[cfg_attr(feature = "std", error("the query to global state failed"))]
    QueryFailedToExecute = 9,
    /// Bad request.
    #[cfg_attr(feature = "std", error("bad request"))]
    BadRequest = 10,
}

impl TryFrom<u8> for ErrorCode {
    type Error = UnknownErrorCode;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::NoError),
            1 => Ok(Self::FunctionIsDisabled),
            2 => Ok(Self::NotFound),
            3 => Ok(Self::RootNotFound),
            4 => Ok(Self::InvalidDeployItemVariant),
            5 => Ok(Self::WasmPreprocessing),
            6 => Ok(Self::InvalidProtocolVersion),
            7 => Ok(Self::InvalidDeploy),
            8 => Ok(Self::InternalError),
            9 => Ok(Self::QueryFailedToExecute),
            10 => Ok(Self::BadRequest),
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
