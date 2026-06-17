use crate::utils::specimen::{Cache, LargestSpecimen, SizeEstimator};
use casper_types::{BlockHash, Transaction, TransactionHash, TransactionId};
use core::fmt;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize, Clone)]
pub(crate) struct AcceptedTransactionId {
    transaction_id: TransactionId,

    block_hash: BlockHash,
}

impl AcceptedTransactionId {}

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

impl AcceptedTransactionId {
    pub(crate) fn transaction_id(&self) -> TransactionId {
        self.transaction_id
    }

    pub(crate) fn block_hash(&self) -> BlockHash {
        self.block_hash
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize, Clone)]
pub(crate) struct AcceptedTransaction {
    /// The transaction that has been accepted by the node gossiping this transaction,
    transaction: Transaction,
    /// The hash of the block the transaction was verified against.
    block_hash: BlockHash,
}

impl Display for AcceptedTransaction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Accepted Transaction({}) with block hash ({})",
            self.transaction, self.block_hash
        )
    }
}

impl AcceptedTransaction {
    pub(crate) fn new(transaction: Transaction, block_hash: BlockHash) -> Self {
        Self {
            transaction,
            block_hash,
        }
    }

    pub(crate) fn block_hash(&self) -> BlockHash {
        self.block_hash
    }

    pub(crate) fn accepted_id(&self) -> AcceptedTransactionId {
        let transaction_id = self.transaction.compute_id();
        AcceptedTransactionId {
            transaction_id,
            block_hash: self.block_hash,
        }
    }

    pub(crate) fn transaction(&self) -> &Transaction {
        &self.transaction
    }
}

impl LargestSpecimen for AcceptedTransactionId {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        let transaction_id = {
            let deploy_hash =
                TransactionHash::Deploy(LargestSpecimen::largest_specimen(estimator, cache));
            let v1_hash = TransactionHash::V1(LargestSpecimen::largest_specimen(estimator, cache));

            let deploy = TransactionId::new(
                deploy_hash,
                LargestSpecimen::largest_specimen(estimator, cache),
            );
            let v1 =
                TransactionId::new(v1_hash, LargestSpecimen::largest_specimen(estimator, cache));

            if estimator.estimate(&deploy) >= estimator.estimate(&v1) {
                deploy
            } else {
                v1
            }
        };

        let block_hash = BlockHash::largest_specimen(estimator, cache);

        Self {
            transaction_id,
            block_hash,
        }
    }
}

impl LargestSpecimen for AcceptedTransaction {
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

        let block_hash = BlockHash::largest_specimen(estimator, cache);

        Self {
            transaction,
            block_hash,
        }
    }
}
