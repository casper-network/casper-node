use std::sync;

use thiserror::Error;

use casper_types::bytesrepr;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("{0}")]
    BytesRepr(bytesrepr::Error),

    #[error("Another thread panicked while holding a lock")]
    Poison,
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
