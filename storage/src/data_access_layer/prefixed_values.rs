//! Support for obtaining all values with a given key prefix.
use crate::{tracking_copy::TrackingCopyError, KeyPrefix};
use casper_types::{Digest, StoredValue};

/// Represents a request to obtain all values with a given key prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixedValuesRequest {
    state_hash: Digest,
    key_prefix: KeyPrefix,
}

impl PrefixedValuesRequest {
    /// Creates new request.
    pub fn new(state_hash: Digest, key_prefix: KeyPrefix) -> Self {
        Self {
            state_hash,
            key_prefix,
        }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns key prefix.
    pub fn key_prefix(&self) -> &KeyPrefix {
        &self.key_prefix
    }
}

/// Represents a result of a `items_by_prefix` request.
#[derive(Debug)]
pub enum PrefixedValuesResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Contains values returned from the global state.
    Success {
        /// The requested prefix.
        key_prefix: KeyPrefix,
        /// Current values.
        values: Vec<StoredValue>,
    },
    /// Failure.
    Failure(TrackingCopyError),
}
