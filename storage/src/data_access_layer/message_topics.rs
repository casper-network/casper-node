use casper_types::{addressable_entity::MessageTopics, Digest, HashAddr};

use crate::tracking_copy::TrackingCopyError;

/// Request for a message topics.
pub struct MessageTopicsRequest {
    state_hash: Digest,
    hash_addr: HashAddr,
}

impl MessageTopicsRequest {
    /// Creates new request object.
    pub fn new(state_hash: Digest, hash_addr: HashAddr) -> Self {
        Self {
            state_hash,
            hash_addr,
        }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns the hash addr.
    pub fn hash_addr(&self) -> HashAddr {
        self.hash_addr
    }
}

/// Result of a global state query request.
#[derive(Debug)]
pub enum MessageTopicsResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Successful query.
    Success {
        /// Stored value under a path.
        message_topics: MessageTopics,
    },
    /// Tracking Copy Error
    Failure(TrackingCopyError),
}
