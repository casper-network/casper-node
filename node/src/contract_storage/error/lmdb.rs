use std::sync;

use lmdb as lmdb_external;
use thiserror::Error;

use types::bytesrepr;

use super::in_memory;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum Error {
    #[error(transparent)]
    Lmdb(#[from] lmdb_external::Error),

    #[error("{0}")]
    BytesRepr(bytesrepr::Error),

    #[error("Another thread panicked while holding a lock")]
    Poison,
}

impl wasmi::HostError for Error {}

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

impl From<in_memory::Error> for Error {
    fn from(error: in_memory::Error) -> Self {
        match error {
            in_memory::Error::BytesRepr(error) => Error::BytesRepr(error),
            in_memory::Error::Poison => Error::Poison,
        }
    }
}
