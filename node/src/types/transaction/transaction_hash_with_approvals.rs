use std::collections::BTreeSet;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{
    DeployApproval, DeployHash, Transaction, TransactionHash, TransactionV1Approval,
    TransactionV1Hash,
};

use super::{FinalizedApprovals, FinalizedDeployApprovals, FinalizedTransactionV1Approvals};

#[allow(missing_docs)]
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransactionHashWithApprovals {
    Deploy {
        deploy_hash: DeployHash,
        approvals: BTreeSet<DeployApproval>,
    },
    V1 {
        transaction_hash: TransactionV1Hash,
        approvals: BTreeSet<TransactionV1Approval>,
    },
}

impl TransactionHashWithApprovals {
    pub(crate) fn new_deploy(deploy_hash: DeployHash, approvals: BTreeSet<DeployApproval>) -> Self {
        Self::Deploy {
            deploy_hash,
            approvals,
        }
    }

    pub(crate) fn new_v1(
        transaction_hash: TransactionV1Hash,
        approvals: BTreeSet<TransactionV1Approval>,
    ) -> Self {
        Self::V1 {
            transaction_hash,
            approvals,
        }
    }

    pub(crate) fn transaction_hash(&self) -> TransactionHash {
        match self {
            TransactionHashWithApprovals::Deploy { deploy_hash, .. } => {
                TransactionHash::from(deploy_hash)
            }
            TransactionHashWithApprovals::V1 {
                transaction_hash, ..
            } => TransactionHash::from(transaction_hash),
        }
    }

    pub(crate) fn into_hash_and_finalized_approvals(self) -> (TransactionHash, FinalizedApprovals) {
        match self {
            TransactionHashWithApprovals::Deploy {
                deploy_hash,
                approvals,
            } => {
                let hash = TransactionHash::from(deploy_hash);
                let approvals =
                    FinalizedApprovals::Deploy(FinalizedDeployApprovals::new(approvals));
                (hash, approvals)
            }
            TransactionHashWithApprovals::V1 {
                transaction_hash,
                approvals,
            } => {
                let hash = TransactionHash::from(transaction_hash);
                let approvals =
                    FinalizedApprovals::V1(FinalizedTransactionV1Approvals::new(approvals));
                (hash, approvals)
            }
        }
    }
}

impl From<&Transaction> for TransactionHashWithApprovals {
    fn from(txn: &Transaction) -> Self {
        match txn {
            Transaction::Deploy(deploy) => TransactionHashWithApprovals::Deploy {
                deploy_hash: *deploy.hash(),
                approvals: deploy.approvals().clone(),
            },
            Transaction::V1(txn) => TransactionHashWithApprovals::V1 {
                transaction_hash: *txn.hash(),
                approvals: txn.approvals().clone(),
            },
        }
    }
}
