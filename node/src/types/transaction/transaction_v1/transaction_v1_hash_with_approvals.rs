use std::collections::BTreeSet;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{TransactionV1Approval, TransactionV1Hash};

/// The hash of a transaction together with signatures approving it for execution.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionV1HashWithApprovals {
    transaction_hash: TransactionV1Hash,
    approvals: BTreeSet<TransactionV1Approval>,
}
impl TransactionV1HashWithApprovals {
    pub(crate) fn new(
        transaction_hash: TransactionV1Hash,
        approvals: BTreeSet<TransactionV1Approval>,
    ) -> Self {
        Self {
            transaction_hash,
            approvals,
        }
    }

    /// Returns the transaction hash.
    pub(crate) fn transaction_hash(&self) -> &TransactionV1Hash {
        &self.transaction_hash
    }

    /// Returns the approvals.
    pub(crate) fn approvals(&self) -> &BTreeSet<TransactionV1Approval> {
        &self.approvals
    }

    /// Returns the approvals.
    pub(crate) fn take_approvals(self) -> BTreeSet<TransactionV1Approval> {
        self.approvals
    }
}
