use casper_types::bytesrepr;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    BytesRepr(#[from] bytesrepr::Error),
    #[error("received request without payload")]
    NoPayload,
    #[error(transparent)]
    BinaryPort(#[from] casper_binary_port::Error),
}
