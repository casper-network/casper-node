//! Support for obtaining all values under the given key tag.
use casper_types::{Digest, KeyTag};

/// Represents a request to obtain all values under the given key tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetAllValuesRequest {
    state_hash: Digest,
    key_tag: KeyTag,
}

impl GetAllValuesRequest {
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
