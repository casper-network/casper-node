use std::sync::PoisonError;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub(crate) enum InMemError {
    #[error("value not found")]
    ValueNotFound,
    #[error("poisoned lock")]
    PoisonedLock,
}

impl<T> From<PoisonError<T>> for InMemError {
    fn from(_error: PoisonError<T>) -> Self {
        InMemError::PoisonedLock
    }
}
