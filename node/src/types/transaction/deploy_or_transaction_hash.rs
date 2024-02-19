use std::hash::Hash;

use datasize::DataSize;
use derive_more::Display;

use casper_types::{Transaction, TransactionHash, TransactionV1Hash};

use super::{transaction_v1::TransactionV1OrTransferV1Hash, DeployOrTransferHash};

#[derive(Copy, Clone, Display, Debug, Ord, PartialOrd, Eq, PartialEq, Hash, DataSize)]
pub(crate) enum DeployOrTransactionHash {
    #[display(fmt = "deploy {}", _0)]
    Deploy(DeployOrTransferHash),
    #[display(fmt = "transaction {}", _0)]
    V1(TransactionV1OrTransferV1Hash),
}

impl DeployOrTransactionHash {
    pub(crate) fn new(transaction: &Transaction) -> Self {
        match transaction {
            Transaction::Deploy(deploy) => DeployOrTransferHash::new(deploy).into(),
            Transaction::V1(transaction) => TransactionV1OrTransferV1Hash::new(transaction).into(),
        }
    }

    pub(crate) fn transaction_hash(&self) -> TransactionHash {
        match self {
            DeployOrTransactionHash::Deploy(deploy) => (*deploy).into(),
            DeployOrTransactionHash::V1(v1) => (*v1).into(),
        }
    }
}

impl From<DeployOrTransferHash> for DeployOrTransactionHash {
    fn from(value: DeployOrTransferHash) -> Self {
        Self::Deploy(value)
    }
}

impl From<TransactionV1OrTransferV1Hash> for DeployOrTransactionHash {
    fn from(value: TransactionV1OrTransferV1Hash) -> Self {
        Self::V1(value)
    }
}

impl From<TransactionV1Hash> for DeployOrTransactionHash {
    fn from(value: TransactionV1Hash) -> Self {
        DeployOrTransactionHash::V1(TransactionV1OrTransferV1Hash::Transaction(value))
    }
}
