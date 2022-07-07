use std::sync;

use lmdb as lmdb_external;
use thiserror::Error;

use casper_hashing::MerkleConstructionError;
use casper_types::bytesrepr;

use crate::storage::global_state::CommitError;

/// Error enum representing possible error states in LMDB interactions.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// LMDB error returned from underlying `lmdb` crate.
    #[error(transparent)]
    Lmdb(#[from] lmdb_external::Error),

    /// (De)serialization error.
    #[error("{0}")]
    BytesRepr(bytesrepr::Error),

    /// Concurrency error.
    #[error("Another thread panicked while holding a lock")]
    Poison,

    /// Error committing to execution engine.
    #[error(transparent)]
    CommitError(#[from] CommitError),

    /// Merkle proof construction error.
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
