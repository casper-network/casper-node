//! A sender's consecutive EVM transactions, gossiped as one item.
//!
//! Casper proposers can only pack a sender's EVM transactions into one block if they hold all of
//! them when they propose, and execution fails any transaction whose nonce is not the sender's
//! next nonce. Gossiping each transaction on its own spreads a sender's run across the network
//! in random order and at different times. A batch keeps the run together: it is gossiped,
//! accepted and handed to the transaction buffer as a unit, so every node that receives it can
//! propose the whole run.
//!
//! Only the batch's transaction IDs are gossiped. A node missing any of the transactions fetches
//! them one at a time and holds them in memory until it has the whole batch, so a batch never has
//! to fit into a single network message.

use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use casper_types::{Transaction, TransactionId};

/// The largest number of transactions an EVM transaction batch can have.
///
/// A node's own limit is the `evm_transaction_batcher.max_batch_size` config value, which cannot
/// exceed this. This bound is enforced when a batch ID is deserialized from a peer, before any
/// config is consulted, so a peer cannot send an arbitrarily long list of IDs.
pub(crate) const EVM_TRANSACTION_BATCH_SIZE_CEILING: usize = 100;

/// The minimum number of transactions in an EVM transaction batch. A single transaction is
/// gossiped as a plain transaction.
pub(crate) const MIN_EVM_TRANSACTION_BATCH_SIZE: usize = 2;

/// The ordered IDs of the transactions in an EVM transaction batch.
///
/// This is the item the batch gossiper gossips; see the module documentation.
#[derive(Clone, DataSize, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec<TransactionId>", into = "Vec<TransactionId>")]
pub(crate) struct EvmTransactionBatchId(Vec<TransactionId>);

impl EvmTransactionBatchId {
    /// The transaction IDs, in nonce order.
    pub(crate) fn transaction_ids(&self) -> &[TransactionId] {
        &self.0
    }
}

impl TryFrom<Vec<TransactionId>> for EvmTransactionBatchId {
    type Error = EvmTransactionBatchError;

    fn try_from(transaction_ids: Vec<TransactionId>) -> Result<Self, Self::Error> {
        check_size(transaction_ids.len())?;
        Ok(EvmTransactionBatchId(transaction_ids))
    }
}

impl From<EvmTransactionBatchId> for Vec<TransactionId> {
    fn from(batch_id: EvmTransactionBatchId) -> Self {
        batch_id.0
    }
}

impl Display for EvmTransactionBatchId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "evm-transaction-batch[{}]",
            self.0
                .iter()
                .map(TransactionId::transaction_hash)
                .join(", ")
        )
    }
}

/// A sender's EVM transactions with consecutive nonces, in nonce order.
///
/// Only ever held in memory: a batch travels between nodes as its [`EvmTransactionBatchId`], and
/// its transactions are fetched one at a time as [`BatchedTransaction`]s.
#[derive(Clone, DataSize, Debug, PartialEq, Eq)]
pub(crate) struct EvmTransactionBatch(Vec<Transaction>);

impl EvmTransactionBatch {
    /// Creates a batch, checking that the transactions are EVM transactions from one sender with
    /// consecutive ascending nonces, and that there are between
    /// [`MIN_EVM_TRANSACTION_BATCH_SIZE`] and [`EVM_TRANSACTION_BATCH_SIZE_CEILING`] of them.
    pub(crate) fn new(transactions: Vec<Transaction>) -> Result<Self, EvmTransactionBatchError> {
        check_size(transactions.len())?;
        let mut previous = None;
        for transaction in &transactions {
            check_follows(previous, transaction)?;
            previous = Some(transaction);
        }
        Ok(EvmTransactionBatch(transactions))
    }

    /// Consumes the batch, returning its transactions in nonce order.
    pub(crate) fn into_transactions(self) -> Vec<Transaction> {
        self.0
    }

    /// The batch's ID.
    pub(crate) fn id(&self) -> EvmTransactionBatchId {
        EvmTransactionBatchId(self.0.iter().map(Transaction::compute_id).collect())
    }
}

impl Display for EvmTransactionBatch {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.id(), formatter)
    }
}

/// Checks that `next` can follow `previous` in a batch: both are EVM transactions from the same
/// sender, and `next`'s nonce is one more than `previous`'s. With no `previous`, only checks that
/// `next` is an EVM transaction.
///
/// Lets a batch's transactions be checked as they are fetched, before the rest are requested.
pub(crate) fn check_follows(
    previous: Option<&Transaction>,
    next: &Transaction,
) -> Result<(), EvmTransactionBatchError> {
    let as_evm = |transaction: &Transaction| {
        transaction
            .as_evm()
            .map(|evm_transaction| (evm_transaction.from(), evm_transaction.nonce()))
            .ok_or(EvmTransactionBatchError::NotEvm(transaction.hash()))
    };
    let (sender, nonce) = as_evm(next)?;
    let Some(previous) = previous else {
        return Ok(());
    };
    let (previous_sender, previous_nonce) = as_evm(previous)?;
    if sender != previous_sender {
        return Err(EvmTransactionBatchError::MixedSenders);
    }
    if previous_nonce.checked_add(1) != Some(nonce) {
        return Err(EvmTransactionBatchError::NonConsecutiveNonces {
            previous: previous_nonce,
            next: nonce,
        });
    }
    Ok(())
}

/// A transaction fetched from a peer for an EVM transaction batch.
///
/// Unlike a fetched [`Transaction`], it is not stored on arrival. The batcher holds a batch's
/// transactions in memory until it has all of them, then passes them to the transaction acceptor,
/// which stores the ones it accepts.
#[derive(Clone, DataSize, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct BatchedTransaction(Transaction);

impl BatchedTransaction {
    pub(crate) fn new(transaction: Transaction) -> Self {
        BatchedTransaction(transaction)
    }

    pub(crate) fn transaction(&self) -> &Transaction {
        &self.0
    }

    pub(crate) fn into_inner(self) -> Transaction {
        self.0
    }
}

impl Display for BatchedTransaction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "batched {}", self.0)
    }
}

fn check_size(size: usize) -> Result<(), EvmTransactionBatchError> {
    if (MIN_EVM_TRANSACTION_BATCH_SIZE..=EVM_TRANSACTION_BATCH_SIZE_CEILING).contains(&size) {
        Ok(())
    } else {
        Err(EvmTransactionBatchError::InvalidSize(size))
    }
}

/// Why an EVM transaction batch is invalid.
#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum EvmTransactionBatchError {
    #[error(
        "EVM transaction batch has {0} transactions; expected between \
        {MIN_EVM_TRANSACTION_BATCH_SIZE} and {EVM_TRANSACTION_BATCH_SIZE_CEILING}"
    )]
    InvalidSize(usize),
    #[error("EVM transaction batch contains non-EVM transaction {0}")]
    NotEvm(casper_types::TransactionHash),
    #[error("EVM transaction batch contains transactions from more than one sender")]
    MixedSenders,
    #[error("EVM transaction batch nonce {next} does not follow nonce {previous}")]
    NonConsecutiveNonces { previous: u64, next: u64 },
}

mod specimen_support {
    use crate::utils::specimen::{vec_of_largest_specimen, Cache, LargestSpecimen, SizeEstimator};

    use super::{BatchedTransaction, EvmTransactionBatchId, EVM_TRANSACTION_BATCH_SIZE_CEILING};

    impl LargestSpecimen for EvmTransactionBatchId {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            EvmTransactionBatchId(vec_of_largest_specimen(
                estimator,
                EVM_TRANSACTION_BATCH_SIZE_CEILING,
                cache,
            ))
        }
    }

    impl LargestSpecimen for BatchedTransaction {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            BatchedTransaction(LargestSpecimen::largest_specimen(estimator, cache))
        }
    }
}
