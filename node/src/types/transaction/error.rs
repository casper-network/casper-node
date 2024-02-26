use std::fmt;

use casper_types::{DeployError, TransactionV1Error};

#[derive(Debug)]
pub(crate) enum TransactionError {
    Deploy(DeployError),
    V1(TransactionV1Error),
}

impl From<TransactionV1Error> for TransactionError {
    fn from(value: TransactionV1Error) -> Self {
        Self::V1(value)
    }
}

impl From<DeployError> for TransactionError {
    fn from(value: DeployError) -> Self {
        Self::Deploy(value)
    }
}

impl fmt::Display for TransactionError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TransactionError::Deploy(deploy) => write!(formatter, "{}", deploy),
            TransactionError::V1(v1) => write!(formatter, "{}", v1),
        }
    }
}
