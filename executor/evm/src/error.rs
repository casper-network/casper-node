//! Error types returned by the Casper EVM executor.

use casper_storage::tracking_copy::TrackingCopyError;
use casper_types::Key;

use crate::BlockHashProviderError;

/// Result type returned by the EVM executor.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors returned by the EVM executor.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// EVM execution is disabled in the chainspec configuration.
    #[error("EVM execution is disabled")]
    Disabled,
    /// Signed EVM transaction does not include an EIP-155 replay-protection chain id.
    #[error("EVM transaction is missing replay-protection chain id")]
    MissingChainId,
    /// Transaction chain id does not match the executor configuration.
    #[error("EVM transaction chain id {actual} does not match configured chain id {expected}")]
    ChainIdMismatch {
        /// Chain id configured in the chainspec.
        expected: u64,
        /// Chain id recovered from the signed transaction.
        actual: u64,
    },
    /// Failed to read from the Casper tracking copy as a revm database.
    #[error(transparent)]
    Database(#[from] DbError),
    /// Failed to translate revm transaction environment.
    #[error("failed to build EVM transaction environment: {0}")]
    Transaction(String),
    /// revm rejected execution before producing state.
    #[error("EVM execution failed: {0}")]
    Revm(String),
    /// Failed to apply EVM state changes to the tracking copy.
    #[error("failed to apply EVM state changes: {0}")]
    State(String),
}

/// Errors emitted by the revm database adapter.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// Failed while reading from the tracking copy.
    #[error(transparent)]
    TrackingCopy(#[from] TrackingCopyError),
    /// The value stored under an EVM key has an unexpected variant.
    #[error("unexpected stored value for {key}: expected {expected}, found {found}")]
    TypeMismatch {
        /// Global-state key that was read.
        key: Box<Key>,
        /// Expected stored-value shape.
        expected: &'static str,
        /// Actual stored-value shape.
        found: String,
    },
    /// A Casper balance does not fit into EVM U256.
    #[error("Casper balance at {key} does not fit into EVM U256")]
    BalanceOverflow {
        /// Balance key that was read.
        key: Box<Key>,
    },
    /// A Casper CLValue failed to decode as a balance.
    #[error("failed to decode Casper balance at {key}: {error}")]
    BalanceDecode {
        /// Balance key that was read.
        key: Box<Key>,
        /// Decode error text.
        error: String,
    },
    /// Failed to resolve a historical block hash for the EVM `BLOCKHASH` opcode.
    #[error("failed to resolve EVM block hash at height {height}: {error}")]
    BlockHash {
        /// Block height requested by the EVM.
        height: u64,
        /// Provider error.
        error: BlockHashProviderError,
    },
}

impl revm::database_interface::DBErrorMarker for DbError {}
