use std::{num::ParseIntError, path::PathBuf};

use humantime::{DurationError, TimestampError};
use jsonrpc_lite::JsonRpc;
use thiserror::Error;

use casper_node::crypto::Error as CryptoError;
use casper_types::{bytesrepr::Error as ToBytesError, UIntParseError, URefFromStrError};

use crate::validation::ValidateResponseError;

/// Crate-wide Result type wrapper.
pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Error that can be returned by `casper-client`.
#[derive(Error, Debug)]
pub enum Error {
    /// Failed to parse a
    /// [`Key`](https://docs.rs/casper-types/latest/casper-types/enum.PublicKey.html) from a
    /// formatted string.
    #[error("failed to parse as a key")]
    FailedToParseKey,

    /// Failed to parse a `URef` from a formatted string.
    #[error("failed to parse '{0}' as a uref: {1:?}")]
    FailedToParseURef(&'static str, URefFromStrError),

    /// Failed to parse an integer from a string.
    #[error("failed to parse '{0}' as an integer: {1:?}")]
    FailedToParseInt(&'static str, ParseIntError),

    /// Failed to parse a `TimeDiff` from a formatted string.
    #[error("failed to parse '{0}' as a time diff: {1:?}")]
    FailedToParseTimeDiff(&'static str, DurationError),

    /// Failed to parse a `Timestamp` from a formatted string.
    #[error("failed to parse '{0}' as a timestamp: {1:?}")]
    FailedToParseTimestamp(&'static str, TimestampError),

    /// Failed to parse a `U128`, `U256` or `U512` from a string.
    #[error("failed to parse '{0}' as U128, U256, or U512: {1:?}")]
    FailedToParseUint(&'static str, UIntParseError),

    /// Failed to get a response from the node.
    #[error("failed to get rpc response: {0}")]
    FailedToGetResponse(reqwest::Error),

    /// Failed to parse the response from the node.
    #[error("failed to parse as json-rpc response: {0}")]
    FailedToParseResponse(reqwest::Error),

    /// Failed to create new key file because it already exists.
    #[error("file already exists: {0:?}")]
    FileAlreadyExists(PathBuf),

    /// Unsupported keygen algorithm.
    #[error("unsupported keygen algorithm: {0}")]
    UnsupportedAlgorithm(String),

    /// JSON-RPC error returned from the node.
    #[error("rpc response is error: {0}")]
    ResponseIsError(#[from] jsonrpc_lite::Error),

    /// Invalid JSON returned from the node.
    #[error("invalid json: {0}")]
    InvalidJson(#[from] serde_json::Error),

    /// Invalid response returned from the node.
    #[error("invalid response: {0:?}")]
    InvalidRpcResponse(JsonRpc),

    /// Failed to send the request to the node.
    #[error("Failed sending {0:?}")]
    FailedSending(JsonRpc),

    /// Context-adding wrapper for `std::io::Error`.
    #[error("io::Error Context: {context} - IoError: {error}")]
    IoError {
        /// Contextual description of where this error occurred including relevant paths,
        /// filenames, etc.
        context: String,
        /// std::io::Error raised during the operation in question.
        error: std::io::Error,
    },

    /// Failed to serialize to bytes.
    #[error("error in to_bytes {0}")]
    ToBytesError(ToBytesError),

    /// Cryptographic error.
    #[error("Crypto error: {0}")]
    CryptoError(#[from] CryptoError),

    /// Invalid `CLValue`.
    #[error("Invalid CLValue error {0}")]
    InvalidCLValue(String),

    /// Invalid argument.
    #[error("Invalid argument '{0}' {1}")]
    InvalidArgument(&'static str, String),

    /// Failed to validate response.
    #[error("Invalid response {0}")]
    InvalidResponse(#[from] ValidateResponseError),

    /// Must call FFI's setup function prior to making ffi calls.
    #[cfg(feature = "ffi")]
    #[error("casper_setup_client() has not been called")]
    FFISetupNotCalled,

    /// Must pass valid pointer values to FFI calls.
    #[cfg(feature = "ffi")]
    #[error("Required argument '{0}' was null")]
    FFIPtrNullButRequired(&'static str),
}

impl From<ToBytesError> for Error {
    fn from(error: ToBytesError) -> Self {
        Error::ToBytesError(error)
    }
}
