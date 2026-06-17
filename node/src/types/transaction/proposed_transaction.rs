use crate::utils::specimen::{Cache, LargestSpecimen, SizeEstimator};
use casper_types::{BlockHash, Transaction, TransactionId};
use core::fmt;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize, Clone)]
pub(crate) struct AcceptedTransactionId {
    transaction_id: TransactionId,

    block_hash: BlockHash,
}

impl Display for AcceptedTransactionId {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "accepted-transaction-id({}, {}, {})",
            self.transaction_id.transaction_hash(),
            self.transaction_id.approvals_hash(),
            self.block_hash
        )
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ProposedTransaction {
    /// The transaction that has been accepted by the node gossiping this transaction,
    transaction: Transaction,
}

impl Display for ProposedTransaction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Accepted Transaction({})", self.transaction,)
    }
}

impl ProposedTransaction {
    pub(crate) fn new(transaction: Transaction) -> Self {
        Self { transaction }
    }

    pub(crate) fn transaction(&self) -> &Transaction {
        &self.transaction
    }
}

impl LargestSpecimen for ProposedTransaction {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        let transaction = {
            let deploy = Transaction::Deploy(LargestSpecimen::largest_specimen(estimator, cache));
            let v1 = Transaction::V1(LargestSpecimen::largest_specimen(estimator, cache));

            if estimator.estimate(&deploy) >= estimator.estimate(&v1) {
                deploy
            } else {
                v1
            }
        };

        Self { transaction }
    }
}
