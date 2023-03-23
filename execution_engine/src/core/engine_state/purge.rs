//! Support for purging leaf nodes from the merkle trie.
use casper_hashing::Digest;
use casper_types::Key;

/// Represents a successfully performed purge.
#[derive(Debug, Clone)]
pub enum PurgeResult {
    /// Root not found.
    RootNotFound,
    /// Key does not exists.
    DoesNotExist,
    /// New state root hash generated after elements were purged.
    Success {
        /// State root hash.
        post_state_hash: Digest,
    },
}

/// Represents the configuration of a protocol upgrade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgeConfig {
    pre_state_hash: Digest,
    keys_to_delete: Vec<Key>,
}

impl PurgeConfig {
    /// Create new upgrade config.
    #[allow(clippy::too_many_arguments)]
    pub fn new(pre_state_hash: Digest, keys_to_purge: Vec<Key>) -> Self {
        PurgeConfig {
            pre_state_hash,
            keys_to_delete: keys_to_purge,
        }
    }

    /// Returns the current state root state hash
    pub fn pre_state_hash(&self) -> Digest {
        self.pre_state_hash
    }

    /// Returns the list of keys to delete.
    pub fn keys_to_delete(&self) -> &[Key] {
        &self.keys_to_delete
    }
}

// /// Represents outcomes of a failed protocol upgrade.
// #[derive(Clone, Error, Debug)]
// pub enum PurgeError {
//     /// Error validating a protocol upgrade config.
//     #[error("Invalid upgrade config")]
//     InvalidPurgeConfig,
//     /// Unable to retrieve a system contract.
//     #[error("Unable to retrieve system contract: {0}")]
//     UnableToRetrieveSystemContract(String),
//     /// Unable to retrieve a system contract package.
//     #[error("Unable to retrieve system contract package: {0}")]
//     UnableToRetrieveSystemContractPackage(String),
//     /// Unable to disable previous version of a system contract.
//     #[error("Failed to disable previous version of system contract: {0}")]
//     FailedToDisablePreviousVersion(String),
//     /// (De)serialization error.
//     #[error("{0}")]
//     Bytesrepr(bytesrepr::Error),
//     /// Failed to create system contract registry.
//     #[error("Failed to insert system contract registry")]
//     FailedToCreateSystemRegistry,
//     /// Failed to migrate global state.
//     #[error("Failed to perform global state migration: {0}")]
//     MigrationRun(#[from] execution::Error),
// }
