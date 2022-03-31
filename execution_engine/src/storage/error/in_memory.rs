use std::sync;

use thiserror::Error;

use casper_hashing::MerkleConstructionError;
use casper_types::bytesrepr;

/// Error enum encapsulating possible errors from in-memory implementation of data storage.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    /// (De)serialization error.
    #[error("{0}")]
    BytesRepr(bytesrepr::Error),

    /// Concurrency error.
    #[error("Another thread panicked while holding a lock")]
    Poison,

    /// Merkle proof construction error
    #[error("{0}")]
    MerkleConstruction(#[from] MerkleConstructionError),
}

impl From<bytesrepr::Error> for Error {
    fn from(error: bytesrepr::Error) -> Self {
        Error::BytesRepr(error)
    }
}

impl<T> From<sync::PoisonError<T>> for Error {
    fn from(_error: sync::PoisonError<T>) -> Self {
        Error::Poison
    }
}
