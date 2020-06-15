use std::{
    error::Error as StdError, fmt::Debug, io, result::Result as StdResult, sync::PoisonError,
};

use thiserror::Error;

use super::in_mem_store::PoisonedLock;

pub(crate) type Result<T> = StdResult<T, Error>;

/// Error returned by the storage component.
#[derive(Debug, Error)]
pub enum Error {
    /// Failed to create the given directory.
    #[error("failed to create {dir}: {source}")]
    CreateDir {
        /// The path of directory which was attempted to be created.
        dir: String,
        /// Underlying IO error.
        source: io::Error,
    },

    /// Failed to serialize data.
    #[error("serialization: {0}")]
    Serialization(#[source] bincode::ErrorKind),

    /// Failed to deserialize data.
    #[error("deserialization: {0}")]
    Deserialization(#[source] bincode::ErrorKind),

    /// Requested value not found.
    #[error("value not found")]
    NotFound,

    /// Internal storage component error.
    #[error("internal: {0}")]
    Internal(Box<dyn StdError + Send + Sync>),
}

impl Error {
    pub(crate) fn from_serialization(error: bincode::ErrorKind) -> Self {
        Error::Serialization(error)
    }

    pub(crate) fn from_deserialization(error: bincode::ErrorKind) -> Self {
        Error::Deserialization(error)
    }
}

impl From<lmdb::Error> for Error {
    fn from(error: lmdb::Error) -> Self {
        Error::Internal(Box::new(error))
    }
}

impl<T> From<PoisonError<T>> for Error {
    fn from(_error: PoisonError<T>) -> Self {
        Error::Internal(Box::new(PoisonedLock {}))
    }
}
