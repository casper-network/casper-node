//! Support for global state queries.
use casper_types::{global_state::TrieMerkleProof, Digest, Key, StoredValue};

use crate::tracking_copy::{TrackingCopyError, TrackingCopyQueryResult};

/// Result of a global state query request.
#[derive(Debug)]
pub enum QueryResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Value not found.
    ValueNotFound(String),
    /// Successful query.
    Success {
        /// Stored value under a path.
        value: Box<StoredValue>,
        /// Merkle proof of the query.
        proofs: Vec<TrieMerkleProof<Key, StoredValue>>,
    },
    /// Tracking Copy Error
    Failure(TrackingCopyError),
}

/// Request for a global state query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryRequest {
    state_hash: Digest,
    key: Key,
    path: Vec<String>,
}

impl QueryRequest {
    /// Creates new request object.
    pub fn new(state_hash: Digest, key: Key, path: Vec<String>) -> Self {
        QueryRequest {
            state_hash,
            key,
            path,
        }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns a key.
    pub fn key(&self) -> Key {
        self.key
    }

    /// Returns a query path.
    pub fn path(&self) -> &[String] {
        &self.path
    }
}

impl From<TrackingCopyQueryResult> for QueryResult {
    fn from(tracking_copy_query_result: TrackingCopyQueryResult) -> Self {
        match tracking_copy_query_result {
            TrackingCopyQueryResult::ValueNotFound(message) => QueryResult::ValueNotFound(message),
            TrackingCopyQueryResult::CircularReference(message) => {
                QueryResult::Failure(TrackingCopyError::CircularReference(message))
            }
            TrackingCopyQueryResult::Success { value, proofs } => {
                let value = Box::new(value);
                QueryResult::Success { value, proofs }
            }
            TrackingCopyQueryResult::DepthLimit { depth } => {
                QueryResult::Failure(TrackingCopyError::QueryDepthLimit { depth })
            }
            TrackingCopyQueryResult::RootNotFound => QueryResult::RootNotFound,
        }
    }
}
