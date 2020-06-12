use std::{fmt::Debug, result::Result as StdResult, sync::PoisonError};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub(crate) type Result<T> = StdResult<T, Error>;

#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub(crate) enum Error {
    #[error("value not found")]
    NotFound,
    #[error("internal: {0}")]
    Internal(String),
}

impl<T> From<PoisonError<T>> for Error {
    fn from(_error: PoisonError<T>) -> Self {
        Error::Internal(String::from("poisoned lock"))
    }
}
