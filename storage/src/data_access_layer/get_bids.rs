//! Support for obtaining current bids from the auction system.
use crate::tracking_copy::TrackingCopyError;

use casper_types::{system::auction::BidKind, Digest};

/// Represents a request to obtain current bids in the auction system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidsRequest {
    state_hash: Digest,
}

impl BidsRequest {
    /// Creates new request.
    pub fn new(state_hash: Digest) -> Self {
        BidsRequest { state_hash }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }
}

/// Represents a result of a `get_bids` request.
#[derive(Debug)]
pub enum BidsResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Contains current bids returned from the global state.
    Success {
        /// Current bids.
        bids: Vec<BidKind>,
    },
    Failure(TrackingCopyError),
}

impl BidsResult {
    /// Returns wrapped [`Vec<BidKind>`] if this represents a successful query result.
    pub fn into_option(self) -> Option<Vec<BidKind>> {
        if let Self::Success { bids } = self {
            Some(bids)
        } else {
            None
        }
    }
}
