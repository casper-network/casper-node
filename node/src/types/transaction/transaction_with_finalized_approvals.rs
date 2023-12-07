use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{
    Deploy, FinalizedDeployApprovals, FinalizedTransactionV1Approvals, Transaction, TransactionV1,
};

/// A transaction combined with a potential set of finalized approvals.
///
/// Represents a transaction, along with a potential set of approvals different from those contained
/// in the transaction itself. If such a set of approvals is present, it indicates that the set
/// contained in the transaction was not the set used to validate the execution of the transaction
/// after consensus.
///
/// A typical case where these can differ is if a transaction is sent with an original set of
/// approvals to the local node, while a second set of approvals makes it to the proposing node. The
/// local node has to adhere to the proposer's approvals to be guaranteed to obtain the same
/// outcome.
#[derive(DataSize, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum TransactionWithFinalizedApprovals {
    Deploy {
        deploy: Deploy,
        finalized_approvals: Option<FinalizedDeployApprovals>,
    },
    V1 {
        transaction: TransactionV1,
        finalized_approvals: Option<FinalizedTransactionV1Approvals>,
    },
}

impl TransactionWithFinalizedApprovals {
    pub(crate) fn new_deploy(
        deploy: Deploy,
        finalized_approvals: Option<FinalizedDeployApprovals>,
    ) -> Self {
        Self::Deploy {
            deploy,
            finalized_approvals,
        }
    }

    pub(crate) fn new_v1(
        transaction: TransactionV1,
        finalized_approvals: Option<FinalizedTransactionV1Approvals>,
    ) -> Self {
        Self::V1 {
            transaction,
            finalized_approvals,
        }
    }

    /// Creates a transaction by potentially substituting the approvals with the finalized
    /// approvals.
    pub(crate) fn into_naive(self) -> Transaction {
        match self {
            TransactionWithFinalizedApprovals::Deploy {
                mut deploy,
                finalized_approvals,
            } => {
                if let Some(finalized_approvals) = finalized_approvals {
                    deploy = deploy.with_approvals(finalized_approvals.into_inner());
                }
                Transaction::from(deploy)
            }
            TransactionWithFinalizedApprovals::V1 {
                mut transaction,
                finalized_approvals,
            } => {
                if let Some(finalized_approvals) = finalized_approvals {
                    transaction = transaction.with_approvals(finalized_approvals.into_inner());
                }
                Transaction::from(transaction)
            }
        }
    }
}
