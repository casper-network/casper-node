use jsonrpc_lite::JsonRpc;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("failed to get rpc response: {0}")]
    FailedToGetResponse(reqwest::Error),

    #[error("failed to parse as json-rpc response: {0}")]
    FailedToParseResponse(reqwest::Error),

    #[error("rpc response is error: {0}")]
    ResponseIsError(jsonrpc_lite::Error),

    #[error("invalid response: {0:?}")]
    InvalidResponse(JsonRpc),
}
