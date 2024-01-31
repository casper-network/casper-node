mod deploy;
mod execution_info;
mod finalized_approvals;
mod transaction_hash_with_approvals;
mod transaction_v1;
mod transaction_with_finalized_approvals;
mod typed_transaction_hash;

use std::hash::Hash;

use datasize::DataSize;
use derive_more::Display;

use casper_types::{Transaction, TransactionHash, TransactionV1Hash};
pub(crate) use deploy::{
    DeployOrTransferHash, DeployWithFinalizedApprovals, FinalizedDeployApprovals, LegacyDeploy,
};
pub(crate) use execution_info::ExecutionInfo;
pub(crate) use finalized_approvals::FinalizedApprovals;
pub use transaction_hash_with_approvals::TransactionHashWithApprovals;
pub(crate) use transaction_v1::FinalizedTransactionV1Approvals;
pub(crate) use transaction_with_finalized_approvals::TransactionWithFinalizedApprovals;
pub(crate) use typed_transaction_hash::TypedTransactionHash;

// TODO[RC]: Move to proper file
#[derive(Copy, Clone, Display, Debug, Ord, PartialOrd, Eq, PartialEq, Hash, DataSize)]
pub(crate) enum DeployOrTransactionHash {
    #[display(fmt = "deploy {}", _0)]
    Deploy(DeployOrTransferHash),
    #[display(fmt = "transaction {}", _0)]
    V1(TransactionV1Hash),
}

impl DeployOrTransactionHash {
    pub(crate) fn new(transaction: &Transaction) -> Self {
        match transaction {
            Transaction::Deploy(deploy) => {
                if deploy.session().is_transfer() {
                    DeployOrTransferHash::Transfer(*deploy.hash()).into()
                } else {
                    DeployOrTransferHash::Deploy(*deploy.hash()).into()
                }
            }
            Transaction::V1(transaction) => Self::V1(*transaction.hash()),
        }
    }

    #[cfg(test)]
    pub(crate) fn transaction_hash(&self) -> TransactionHash {
        match self {
            DeployOrTransactionHash::Deploy(deploy) => (*deploy).into(),
            DeployOrTransactionHash::V1(v1) => v1.into(),
        }
    }
}

impl From<DeployOrTransferHash> for DeployOrTransactionHash {
    fn from(value: DeployOrTransferHash) -> Self {
        Self::Deploy(value)
    }
}

impl From<TransactionV1Hash> for DeployOrTransactionHash {
    fn from(value: TransactionV1Hash) -> Self {
        Self::V1(value)
    }
}

impl From<DeployOrTransactionHash> for TransactionHash {
    fn from(value: DeployOrTransactionHash) -> Self {
        match value {
            DeployOrTransactionHash::Deploy(deploy) => TransactionHash::Deploy(deploy.into()),
            DeployOrTransactionHash::V1(v1) => TransactionHash::V1(v1),
        }
    }
}
