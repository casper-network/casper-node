use datasize::DataSize;
use derive_more::Display;
use serde::{Deserialize, Serialize};

use casper_types::{TransactionHash, TransactionV1, TransactionV1Hash};

// TODO[RC]: To separate file
/// The [`TransactionHash`] stored in a way distinguishing between V1 transactions and V1 transfers.
#[derive(
    Copy,
    Clone,
    DataSize,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
    Debug,
    Display,
)]
#[serde(deny_unknown_fields)]
pub(crate) enum TransactionV1OrTransferV1Hash {
    /// Hash of a transaction.
    #[display(fmt = "transaction {}", _0)]
    Transaction(TransactionV1Hash),
    /// Hash of a transfer.
    #[display(fmt = "transfer V1 {}", _0)]
    Transfer(TransactionV1Hash),
}

impl TransactionV1OrTransferV1Hash {
    /// Returns the hash of `transaction` wrapped in `TransactionV1OrTransferV1Hash`.
    pub(crate) fn new(transaction: &TransactionV1) -> TransactionV1OrTransferV1Hash {
        if transaction.is_transfer() {
            TransactionV1OrTransferV1Hash::Transfer(*transaction.hash())
        } else {
            TransactionV1OrTransferV1Hash::Transaction(*transaction.hash())
        }
    }
}

impl From<TransactionV1OrTransferV1Hash> for TransactionV1Hash {
    fn from(dt_hash: TransactionV1OrTransferV1Hash) -> TransactionV1Hash {
        match dt_hash {
            TransactionV1OrTransferV1Hash::Transaction(hash) => hash,
            TransactionV1OrTransferV1Hash::Transfer(hash) => hash,
        }
    }
}

impl From<TransactionV1OrTransferV1Hash> for TransactionHash {
    fn from(dt_hash: TransactionV1OrTransferV1Hash) -> TransactionHash {
        match dt_hash {
            TransactionV1OrTransferV1Hash::Transaction(hash) => TransactionHash::V1(hash),
            TransactionV1OrTransferV1Hash::Transfer(hash) => TransactionHash::V1(hash),
        }
    }
}
