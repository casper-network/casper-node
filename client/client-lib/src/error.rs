use jsonrpc_lite::JsonRpc;
use thiserror::Error;

use casper_node::crypto::Error as CryptoError;
use casper_types::bytesrepr::Error as ToBytesError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("failed to get rpc response: {0}")]
    FailedToGetResponse(reqwest::Error),

    #[error("failed to parse as json-rpc response: {0}")]
    FailedToParseResponse(reqwest::Error),

    #[error("rpc response is error: {0}")]
    ResponseIsError(#[from] jsonrpc_lite::Error),

    #[error("invalid json: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("invalid response: {0:?}")]
    InvalidResponse(JsonRpc),

    #[error("io::Error {0:?}")]
    IoError(#[from] std::io::Error),

    #[error("error in to_bytes {0:?}")]
    ToBytesError(ToBytesError),

    #[error("crypto error {0:?}")]
    CryptoError(#[from] CryptoError),
}

impl From<ToBytesError> for Error {
    fn from(e: ToBytesError) -> Self {
        Error::ToBytesError(e)
    }
}
