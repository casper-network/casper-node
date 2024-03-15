use crate::InvalidDeploy;
use core::fmt::{Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;

#[cfg(feature = "std")]
use serde::Serialize;
#[cfg(feature = "std")]
use std::error::Error as StdError;

pub use crate::transaction::transaction_v1::InvalidTransactionV1;

/// A representation of the way in which a transaction failed validation checks.
#[derive(Clone, Eq, PartialEq, Debug)]
#[cfg_attr(feature = "std", derive(Serialize))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[non_exhaustive]
pub enum InvalidTransaction {
    /// Legacy deploys.
    Deploy(InvalidDeploy),
    /// V1 transactions.
    V1(InvalidTransactionV1),
}

impl From<InvalidDeploy> for InvalidTransaction {
    fn from(value: InvalidDeploy) -> Self {
        Self::Deploy(value)
    }
}

impl From<InvalidTransactionV1> for InvalidTransaction {
    fn from(value: InvalidTransactionV1) -> Self {
        Self::V1(value)
    }
}

#[cfg(feature = "std")]
impl StdError for InvalidTransaction {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            InvalidTransaction::Deploy(deploy) => deploy.source(),
            InvalidTransaction::V1(v1) => v1.source(),
        }
    }
}

impl Display for InvalidTransaction {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            InvalidTransaction::Deploy(inner) => Display::fmt(inner, f),
            InvalidTransaction::V1(inner) => Display::fmt(inner, f),
        }
    }
}
