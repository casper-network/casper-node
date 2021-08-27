use casper_types::{system::auction::Bids, Digest, Key, StoredValue};

use crate::{
    core::tracking_copy::TrackingCopyQueryResult, storage::trie::merkle_proof::TrieMerkleProof,
};

#[derive(Debug)]
pub enum QueryResult {
    RootNotFound,
    ValueNotFound(String),
    CircularReference(String),
    DepthLimit {
        depth: u64,
    },
    Success {
        value: Box<StoredValue>,
        proofs: Vec<TrieMerkleProof<Key, StoredValue>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryRequest {
    state_hash: Digest,
    key: Key,
    path: Vec<String>,
}

impl QueryRequest {
    pub fn new(state_hash: Digest, key: Key, path: Vec<String>) -> Self {
        QueryRequest {
            state_hash,
            key,
            path,
        }
    }

    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    pub fn key(&self) -> Key {
        self.key
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetBidsRequest {
    state_hash: Digest,
}

impl GetBidsRequest {
    pub fn new(state_hash: Digest) -> Self {
        GetBidsRequest { state_hash }
    }

    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }
}

#[derive(Debug)]
pub enum GetBidsResult {
    RootNotFound,
    Success { bids: Bids },
}

impl GetBidsResult {
    pub fn success(bids: Bids) -> Self {
        GetBidsResult::Success { bids }
    }

    pub fn bids(&self) -> Option<&Bids> {
        match self {
            GetBidsResult::RootNotFound => None,
            GetBidsResult::Success { bids } => Some(bids),
        }
    }
}
