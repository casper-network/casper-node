#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use std::collections::BTreeSet;

#[cfg(any(feature = "testing", test))]
use super::FinalizedApprovals;
#[cfg(any(feature = "testing", test))]
use crate::TransactionApproval;
use crate::{Deploy, Transaction, TransactionV1};

use super::{deploy::FinalizedDeployApprovals, transaction_v1::FinalizedTransactionV1Approvals};

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
#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(any(feature = "std", test), derive(Serialize, Deserialize))]
pub enum TransactionWithFinalizedApprovals {
    /// A deploy combined with a potential set of finalized approvals.
    Deploy {
        /// The deploy.
        deploy: Deploy,
        /// The set of finalized approvals if they exist.
        finalized_approvals: Option<FinalizedDeployApprovals>,
    },
    /// A v1 transaction combined with a potential set of finalized approvals.
    V1 {
        /// The Transaction.
        transaction: TransactionV1,
        /// The set of finalized approvals if they exist.
        finalized_approvals: Option<FinalizedTransactionV1Approvals>,
    },
}

impl TransactionWithFinalizedApprovals {
    /// Creates a [`TransactionWithFinalizedApprovals`] for a [`Deploy`]
    pub fn new_deploy(
        deploy: Deploy,
        finalized_approvals: Option<FinalizedDeployApprovals>,
    ) -> Self {
        Self::Deploy {
            deploy,
            finalized_approvals,
        }
    }

    /// Creates a [`TransactionWithFinalizedApprovals`] for a [`TransactionV1`]
    pub fn new_v1(
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
    pub fn into_naive(self) -> Transaction {
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
    pub fn discard_finalized_approvals(self) -> Transaction {
        match self {
            TransactionWithFinalizedApprovals::Deploy { deploy, .. } => Transaction::Deploy(deploy),
            TransactionWithFinalizedApprovals::V1 { transaction, .. } => {
                Transaction::V1(transaction)
            }
        }
    }

    /// Returns the original transaction approvals.
    #[cfg(any(feature = "testing", test))]
    pub fn original_approvals(&self) -> BTreeSet<TransactionApproval> {
        match self {
            TransactionWithFinalizedApprovals::Deploy { deploy, .. } => {
                deploy.approvals().iter().map(Into::into).collect()
            }
            TransactionWithFinalizedApprovals::V1 { transaction, .. } => {
                transaction.approvals().iter().map(Into::into).collect()
            }
        }
    }

    /// Returns the finalized transaction approvals.
    #[cfg(any(feature = "testing", test))]
    pub fn finalized_approvals(&self) -> Option<FinalizedApprovals> {
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
