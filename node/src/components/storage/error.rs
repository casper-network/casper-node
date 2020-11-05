use std::{error::Error as StdError, fmt::Debug, io, result::Result as StdResult};

use thiserror::Error;

use casper_types::bytesrepr;

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
    Serialization(String),

    /// Failed to deserialize data.
    #[error("deserialization: {0}")]
    Deserialization(String),

    /// Internal storage component error.
    #[error("internal: {0}")]
    Internal(Box<dyn StdError + Send + Sync>),
}

impl Error {
    pub(crate) fn from_serialization(error: bytesrepr::Error) -> Self {
        Error::Serialization(error.to_string())
    }

    pub(crate) fn from_deserialization(error: bytesrepr::Error) -> Self {
        Error::Deserialization(error.to_string())
    }
}

impl From<lmdb::Error> for Error {
    fn from(error: lmdb::Error) -> Self {
        Error::Internal(Box::new(error))
    }
}
