use std::{error::Error as StdError, fmt::Debug, io, result::Result as StdResult};

use thiserror::Error;

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
