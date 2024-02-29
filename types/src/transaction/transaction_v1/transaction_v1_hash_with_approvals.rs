use alloc::collections::BTreeSet;

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{TransactionV1Approval, TransactionV1Hash};

/// The hash of a transaction together with signatures approving it for execution.
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionV1HashWithApprovals {
    transaction_hash: TransactionV1Hash,
    approvals: BTreeSet<TransactionV1Approval>,
}
impl TransactionV1HashWithApprovals {
    /// Creates a new `TransactionV1HashWithApprovals` instance.
    pub fn new(
        transaction_hash: TransactionV1Hash,
        approvals: BTreeSet<TransactionV1Approval>,
    ) -> Self {
        Self {
            transaction_hash,
            approvals,
        }
    }

    /// Returns the transaction hash.
    pub fn transaction_hash(&self) -> &TransactionV1Hash {
        &self.transaction_hash
    }

    /// Returns the approvals.
    pub fn approvals(&self) -> &BTreeSet<TransactionV1Approval> {
        &self.approvals
    }

    /// Returns the approvals.
    pub fn take_approvals(self) -> BTreeSet<TransactionV1Approval> {
        self.approvals
    }
}
