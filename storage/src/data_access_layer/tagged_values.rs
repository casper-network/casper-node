//! Support for obtaining all values under the given key tag.
use crate::tracking_copy::TrackingCopyError;
use casper_types::{Digest, KeyTag, StoredValue};

/// Tagged values selector.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TaggedValuesSelection {
    /// All values under the specified key tag.
    All(KeyTag),
}

/// Represents a request to obtain all values under the given key tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedValuesRequest {
    state_hash: Digest,
    selection: TaggedValuesSelection,
}

impl TaggedValuesRequest {
    /// Creates new request.
    pub fn new(state_hash: Digest, selection: TaggedValuesSelection) -> Self {
        Self {
            state_hash,
            selection,
        }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns key tag.
    pub fn key_tag(&self) -> KeyTag {
        match self.selection {
            TaggedValuesSelection::All(key_tag) => key_tag,
        }
    }

    /// Returns selection criteria.
    pub fn selection(&self) -> TaggedValuesSelection {
        self.selection
    }
}

/// Represents a result of a `get_all_values` request.
#[derive(Debug)]
pub enum TaggedValuesResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Contains values returned from the global state.
    Success {
        /// The requested selection.
        selection: TaggedValuesSelection,
        /// Current values.
        values: Vec<StoredValue>,
    },
    /// Tagged value failure.
    Failure(TrackingCopyError),
}
