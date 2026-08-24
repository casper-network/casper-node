//! Error types returned by the Casper EVM executor.

use casper_storage::{block_store::BlockStoreError, tracking_copy::TrackingCopyError};
use casper_types::Key;

use crate::account_state::AccountStorageError;

/// Result type returned by the EVM executor.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors returned by the EVM executor.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// EVM execution is disabled in the chainspec configuration.
    #[error("EVM execution is disabled")]
    Disabled,
    /// EVM wei-to-mote conversion ratio is invalid.
    #[error("EVM wei_per_mote must be greater than zero")]
    InvalidWeiPerMote,
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
    /// A Casper CLValue failed to decode as an expected EVM account field.
    #[error("failed to decode {expected} at {key}: {error}")]
    ValueDecode {
        /// Global-state key that was read.
        key: Box<Key>,
        /// Expected decoded type.
        expected: &'static str,
        /// Decode error text.
        error: String,
    },
    /// A Casper balance, after scaling from motes to wei, does not fit into EVM U256.
    #[error("Casper balance at {key}, scaled to wei, does not fit into EVM U256")]
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
    /// Failed to resolve a historical EVM block hash from the block store.
    #[error("failed to resolve EVM block hash at height {height}: {error}")]
    BlockHash {
        /// Block height requested by the EVM.
        height: u64,
        /// Block store error.
        error: BlockStoreError,
    },
}

impl From<AccountStorageError> for DbError {
    fn from(error: AccountStorageError) -> Self {
        match error {
            AccountStorageError::TrackingCopy(error) => DbError::TrackingCopy(error),
            AccountStorageError::TypeMismatch {
                key,
                expected,
                found,
            } => DbError::TypeMismatch {
                key,
                expected,
                found,
            },
            AccountStorageError::Decode {
                key,
                expected,
                error,
            } => DbError::ValueDecode {
                key,
                expected,
                error,
            },
            AccountStorageError::MissingAccount {
                identity_key,
                account_key,
            } => DbError::TypeMismatch {
                key: identity_key,
                expected: "existing linked account",
                found: format!("missing {account_key}"),
            },
        }
    }
}

impl From<AccountStorageError> for Error {
    fn from(error: AccountStorageError) -> Self {
        Error::State(error.to_string())
    }
}

impl revm::database_interface::DBErrorMarker for DbError {}
