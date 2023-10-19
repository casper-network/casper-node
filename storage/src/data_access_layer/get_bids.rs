//! Support for obtaining current bids from the auction system.
use crate::{global_state::error::Error as GlobalStateError, tracking_copy::TrackingCopyError};

use casper_types::{system::auction::BidKind, Digest, Key};

/// Represents a request to obtain current bids in the auction system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetBidsRequest {
    state_hash: Digest,
}

impl GetBidsRequest {
    /// Creates new request.
    pub fn new(state_hash: Digest) -> Self {
        GetBidsRequest { state_hash }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }
}

#[derive(Debug)]
pub enum GetBidsError {
    GlobalState(GlobalStateError),
    TrackingCopyError(TrackingCopyError),
    MissingBid(Key),
    InvalidStoredValueVariant(Key),
}

/// Represents a result of a `get_bids` request.
#[derive(Debug)]
pub enum GetBidsResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Contains current bids returned from the global state.
    Success {
        /// Current bids.
        bids: Vec<BidKind>,
    },
    Failure(GetBidsError),
}

impl GetBidsResult {
    /// Returns wrapped [`Vec<BidKind>`] if this represents a successful query result.
    pub fn into_success(self) -> Option<Vec<BidKind>> {
        if let Self::Success { bids } = self {
            Some(bids)
        } else {
            None
        }
    }
}
