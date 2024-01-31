use datasize::DataSize;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use std::collections::BTreeSet;

#[cfg(test)]
use super::FinalizedApprovals;
#[cfg(test)]
use casper_types::TransactionApproval;
use casper_types::{Deploy, Transaction, TransactionV1};

use crate::types::{FinalizedDeployApprovals, FinalizedTransactionV1Approvals};

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

    /// Extracts the original transaction by discarding the finalized approvals.
    pub(crate) fn discard_finalized_approvals(self) -> Transaction {
        match self {
            TransactionWithFinalizedApprovals::Deploy { deploy, .. } => Transaction::Deploy(deploy),
            TransactionWithFinalizedApprovals::V1 { transaction, .. } => {
                Transaction::V1(transaction)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn original_approvals(&self) -> BTreeSet<TransactionApproval> {
        match self {
            TransactionWithFinalizedApprovals::Deploy { deploy, .. } => {
                deploy.approvals().iter().map(Into::into).collect()
            }
            TransactionWithFinalizedApprovals::V1 { transaction, .. } => {
                transaction.approvals().iter().map(Into::into).collect()
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn finalized_approvals(&self) -> Option<FinalizedApprovals> {
        match self {
            TransactionWithFinalizedApprovals::Deploy {
                finalized_approvals,
                ..
            } => finalized_approvals.clone().map(Into::into),
            TransactionWithFinalizedApprovals::V1 {
                finalized_approvals,
                ..
            } => finalized_approvals.clone().map(Into::into),
        }
    }
}
