use jsonrpc_lite::JsonRpc;
use thiserror::Error;

use casper_node::crypto::Error as CryptoError;
use casper_types::bytesrepr::Error as ToBytesError;

/// Crate-wide Result type wrapper.
pub type Result<T> = std::result::Result<T, Error>;

/// Error that can be returned by `casper-client`.
#[derive(Error, Debug)]
pub enum Error {
    /// Failed to get a response from the server.
    #[error("failed to get rpc response: {0}")]
    FailedToGetResponse(reqwest::Error),

    /// Failed to parse the response from the server.
    #[error("failed to parse as json-rpc response: {0}")]
    FailedToParseResponse(reqwest::Error),

    /// JSON-RPC error returned from the server.
    #[error("rpc response is error: {0}")]
    ResponseIsError(#[from] jsonrpc_lite::Error),

    /// Invalid JSON returned from the server.
    #[error("invalid json: {0}")]
    InvalidJson(#[from] serde_json::Error),

    /// Invalid response returned from the server.
    #[error("invalid response: {0:?}")]
    InvalidResponse(JsonRpc),

    /// Failed to send the request to the server.
    #[error("Failed sending {0:?}")]
    FailedSending(JsonRpc),

    /// Wrapper for std::io::Error
    #[error("io::Error {0}")]
    IoError(#[from] std::io::Error),

    /// Failed to serialize to bytes.
    #[error("error in to_bytes {0}")]
    ToBytesError(ToBytesError),

    /// Cryptographic error.
    #[error("crypto error {0}")]
    CryptoError(#[from] CryptoError),
}

impl From<ToBytesError> for Error {
    fn from(e: ToBytesError) -> Self {
        Error::ToBytesError(e)
    }
}
