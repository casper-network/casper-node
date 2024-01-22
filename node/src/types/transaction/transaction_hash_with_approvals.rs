use std::collections::BTreeSet;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{
    DeployApproval, DeployHash, Transaction, TransactionApproval, TransactionHash,
    TransactionV1Approval, TransactionV1Hash,
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

    // TODO[RC]: Rework so the panics are impossible
    pub(crate) fn new_from_hash_and_approvals(
        hash: &TransactionHash,
        approvals: &BTreeSet<TransactionApproval>,
    ) -> Self {
        match hash {
            TransactionHash::Deploy(deploy_hash) => {
                let approvals: BTreeSet<_> = approvals
                    .iter()
                    .map(|approval| match approval {
                        TransactionApproval::Deploy(approval) => approval.clone(),
                        TransactionApproval::V1(_) => panic!("Unexpected Deploy with V1 approval"),
                    })
                    .collect();
                Self::new_deploy(*deploy_hash, approvals)
            }
            TransactionHash::V1(v1_hash) => {
                let approvals: BTreeSet<_> = approvals
                    .iter()
                    .map(|approval| match approval {
                        TransactionApproval::Deploy(_) => {
                            panic!("Unexpected Transaction with Deploy approval")
                        }
                        TransactionApproval::V1(approval) => approval.clone(),
                    })
                    .collect();

                Self::new_v1(*v1_hash, approvals.clone())
            }
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

    pub(crate) fn approvals_count(&self) -> usize {
        match self {
            TransactionHashWithApprovals::Deploy { approvals, .. } => approvals.len(),
            TransactionHashWithApprovals::V1 { approvals, .. } => approvals.len(),
        }
    }

    /// Returns the approvals.
    // TODO[RC]: It's been returning a reference previously
    pub(crate) fn approvals(&self) -> BTreeSet<TransactionApproval> {
        match self {
            TransactionHashWithApprovals::Deploy {
                deploy_hash,
                approvals,
            } => approvals.into_iter().map(Into::into).collect(),
            TransactionHashWithApprovals::V1 {
                transaction_hash,
                approvals,
            } => approvals.into_iter().map(Into::into).collect(),
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
