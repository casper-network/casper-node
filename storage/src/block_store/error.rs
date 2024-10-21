use casper_types::{BlockHash, EraId, TransactionHash};
use std::fmt::Debug;
use thiserror::Error;

/// Block store error.
#[derive(Debug, Error)]
pub enum BlockStoreError {
    /// Found a duplicate block entry of the specified height.
    #[error("duplicate entries for block at height {height}: {first} / {second}")]
    DuplicateBlock {
        /// Height at which duplicate was found.
        height: u64,
        /// First block hash encountered at `height`.
        first: BlockHash,
        /// Second block hash encountered at `height`.
        second: BlockHash,
    },
    /// Found a duplicate switch-block entry of the specified height.
    #[error("duplicate entries for switch block at era id {era_id}: {first} / {second}")]
    DuplicateEraId {
        /// Era ID at which duplicate was found.
        era_id: EraId,
        /// First block hash encountered at `era_id`.
        first: BlockHash,
        /// Second block hash encountered at `era_id`.
        second: BlockHash,
    },
    /// Found a duplicate transaction entry.
    #[error("duplicate entries for blocks for transaction {transaction_hash}: {first} / {second}")]
    DuplicateTransaction {
        /// Transaction hash at which duplicate was found.
        transaction_hash: TransactionHash,
        /// First block hash encountered at `transaction_hash`.
        first: BlockHash,
        /// Second block hash encountered at `transaction_hash`.
        second: BlockHash,
    },
    /// Internal error.
    #[error("internal database error: {0}")]
    InternalStorage(Box<dyn std::error::Error + Send + Sync>),
    /// The operation is unsupported.
    #[error("unsupported operation")]
    UnsupportedOperation,
}
