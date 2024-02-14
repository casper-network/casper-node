use crate::tracking_copy::TrackingCopyError;
use casper_types::{AddressableEntity, Digest, Key};

/// Represents a request to obtain an addressable entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressableEntityRequest {
    state_hash: Digest,
    key: Key,
}

impl AddressableEntityRequest {
    /// Creates new request.
    pub fn new(state_hash: Digest, key: Key) -> Self {
        AddressableEntityRequest { state_hash, key }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns key.
    pub fn key(&self) -> Key {
        self.key
    }
}

/// Represents a result of a `addressable_entity` request.
#[derive(Debug)]
pub enum AddressableEntityResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Value not found.
    ValueNotFound(String),
    /// Contains an addressable entity from global state.
    Success {
        /// An addressable entity.
        entity: AddressableEntity,
    },
    Failure(TrackingCopyError),
}

impl AddressableEntityResult {
    /// Returns wrapped addressable entity if this represents a successful query result.
    pub fn into_option(self) -> Option<AddressableEntity> {
        if let Self::Success { entity } = self {
            Some(entity)
        } else {
            None
        }
    }
}
