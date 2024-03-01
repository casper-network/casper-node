//! Support for obtaining all values under the given key tag.
use crate::tracking_copy::TrackingCopyError;
use casper_types::{Digest, KeyTag, StoredValue};

/// Represents a request to obtain all values under the given key tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllValuesRequest {
    state_hash: Digest,
    key_tag: KeyTag,
}

impl AllValuesRequest {
    /// Creates new request.
    pub fn new(state_hash: Digest, key_tag: KeyTag) -> Self {
        Self {
            state_hash,
            key_tag,
        }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns key tag.
    pub fn key_tag(&self) -> KeyTag {
        self.key_tag
    }
}
/// Represents a result of a `get_all_values` request.
#[derive(Debug)]
pub enum AllValuesResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Contains values returned from the global state.
    Success {
        /// Current values.
        values: Vec<StoredValue>,
    },
    Failure(TrackingCopyError),
}
