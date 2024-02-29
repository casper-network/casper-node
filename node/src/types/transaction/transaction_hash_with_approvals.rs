use std::collections::BTreeSet;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{
    DeployApproval, DeployHash, FinalizedApprovals, FinalizedDeployApprovals,
    FinalizedTransactionV1Approvals, Transaction, TransactionApproval, TransactionHash,
    TransactionV1Approval, TransactionV1Hash, TransactionV1HashWithApprovals,
};
use tracing::error;

use super::DeployHashWithApprovals;

#[allow(missing_docs)]
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransactionHashWithApprovals {
    Deploy {
        deploy_hash: DeployHash,
        approvals: BTreeSet<DeployApproval>,
    },
    V1(TransactionV1HashWithApprovals),
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
        Self::V1(TransactionV1HashWithApprovals::new(
            transaction_hash,
            approvals,
        ))
    }

    pub(crate) fn new_from_hash_and_approvals(
        hash: &TransactionHash,
        approvals: &BTreeSet<TransactionApproval>,
    ) -> Self {
        match hash {
            TransactionHash::Deploy(deploy_hash) => {
                let approvals: BTreeSet<_> = approvals
                    .iter()
                    .filter_map(|approval| match approval {
                        TransactionApproval::Deploy(approval) => Some(approval.clone()),
                        TransactionApproval::V1(_) => {
                            error!("can not add 'transaction' approval to 'legacy deploy'");
                            None
                        }
                    })
                    .collect();
                Self::new_deploy(*deploy_hash, approvals)
            }
            TransactionHash::V1(v1_hash) => {
                let approvals: BTreeSet<_> = approvals
                    .iter()
                    .filter_map(|approval| match approval {
                        TransactionApproval::Deploy(_) => {
                            error!("can not add 'legacy deploy' approval to 'transaction'");
                            None
                        }
                        TransactionApproval::V1(approval) => Some(approval.clone()),
                    })
                    .collect();
                Self::new_v1(*v1_hash, approvals)
            }
        }
    }

    pub(crate) fn transaction_hash(&self) -> TransactionHash {
        match self {
            TransactionHashWithApprovals::Deploy { deploy_hash, .. } => {
                TransactionHash::from(deploy_hash)
            }
            TransactionHashWithApprovals::V1(thwa) => {
                TransactionHash::from(thwa.transaction_hash())
            }
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
            TransactionHashWithApprovals::V1(thwa) => {
                let hash = TransactionHash::from(thwa.transaction_hash());
                let approvals = FinalizedApprovals::V1(FinalizedTransactionV1Approvals::new(
                    thwa.take_approvals(),
                ));
                (hash, approvals)
            }
        }
    }

    /// Returns the approvals.
    pub(crate) fn approvals(&self) -> BTreeSet<TransactionApproval> {
        match self {
            TransactionHashWithApprovals::Deploy { approvals, .. } => {
                approvals.iter().map(Into::into).collect()
            }
            TransactionHashWithApprovals::V1(thwa) => {
                thwa.approvals().iter().map(Into::into).collect()
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
            Transaction::V1(txn) => TransactionHashWithApprovals::V1(
                TransactionV1HashWithApprovals::new(*txn.hash(), txn.approvals().clone()),
            ),
        }
    }
}

impl From<DeployHashWithApprovals> for TransactionHashWithApprovals {
    fn from(deploy: DeployHashWithApprovals) -> Self {
        TransactionHashWithApprovals::new_deploy(*deploy.deploy_hash(), deploy.approvals().clone())
    }
}
