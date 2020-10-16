use std::sync;

use thiserror::Error;

use casper_types::bytesrepr;

use crate::storage::trie::merkle_proof;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("{0}")]
    BytesRepr(bytesrepr::Error),

    #[error("Another thread panicked while holding a lock")]
    Poison,

    #[error(
    "`hole_index` with value {hole_index:?} must be less than RADIX (proof step {proof_step_index:?})"
    )]
    HoleIndexOutOfRange {
        hole_index: u8,
        proof_step_index: u8,
    },
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

impl From<merkle_proof::Error> for Error {
    fn from(error: merkle_proof::Error) -> Self {
        match error {
            merkle_proof::Error::HoleIndexOutOfRange {
                hole_index,
                proof_step_index,
            } => Error::HoleIndexOutOfRange {
                hole_index,
                proof_step_index,
            },
            merkle_proof::Error::BytesRepr(error) => Error::BytesRepr(error),
        }
    }
}
