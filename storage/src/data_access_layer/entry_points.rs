use crate::tracking_copy::TrackingCopyError;
use casper_types::{Digest, Key, EntryPointValue, EntryPoint};

/// Represents a request to obtain entry points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPointsRequest {
    state_hash: Digest,
    key: Key,
}

impl EntryPointsRequest {
    /// Creates new request.
    pub fn new(state_hash: Digest, key: Key) -> Self {
        EntryPointsRequest { state_hash, key }
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

/// Represents a result of a `entry_points` request.
#[derive(Debug)]
pub enum EntryPointsResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Value not found.
    ValueNotFound(String),
    /// Contains an addressable entity from global state.
    Success {
        /// An addressable entity.
        entry_points: EntryPointValue,
    },
    Failure(TrackingCopyError),
}

impl EntryPointsResult {
    /// Returns the result based on a particular variant of entrypoint
    pub fn into_v1_entry_point(self) -> Option<EntryPoint> {
        if let Self::Success { entry_points } = self {
            match entry_points {
                EntryPointValue::V1CasperVm(entry_point) => { Some(entry_point) }
                EntryPointValue::V2CasperVm(_) => None,
            }
        } else {
            None
        }
    }
}
