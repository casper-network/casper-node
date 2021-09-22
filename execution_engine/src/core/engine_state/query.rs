//! Support for global state queries.
use casper_hashing::Digest;
use casper_types::{Key, StoredValue};

use crate::{
    core::tracking_copy::TrackingCopyQueryResult, storage::trie::merkle_proof::TrieMerkleProof,
};

/// Result of a global state query request.
#[derive(Debug)]
pub enum QueryResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Value not found.
    ValueNotFound(String),
    /// Circular reference error.
    CircularReference(String),
    /// Depth limit reached.
    DepthLimit {
        /// Current depth limit.
        depth: u64,
    },
    /// Successful query.
    Success {
        /// Stored value under a path.
        value: Box<StoredValue>,
        /// Merkle proof of the query.
        proofs: Vec<TrieMerkleProof<Key, StoredValue>>,
    },
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
                QueryResult::CircularReference(message)
            }
            TrackingCopyQueryResult::Success { value, proofs } => {
                let value = Box::new(value);
                QueryResult::Success { value, proofs }
            }
            TrackingCopyQueryResult::DepthLimit { depth } => QueryResult::DepthLimit { depth },
        }
    }
}
