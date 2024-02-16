use crate::tracking_copy::TrackingCopyError;
use casper_types::{Digest, U512};

/// Request for total supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TotalSupplyRequest {
    state_hash: Digest,
}

impl TotalSupplyRequest {
    /// Creates an instance of TotalSupplyRequest.
    pub fn new(state_hash: Digest) -> Self {
        TotalSupplyRequest { state_hash }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }
}

/// Represents a result of a `total_supply` request.
#[derive(Debug)]
pub enum TotalSupplyResult {
    /// Invalid state root hash.
    RootNotFound,
    /// The mint is not found.
    MintNotFound,
    /// Value not found.
    ValueNotFound(String),
    /// The total supply at the specified state hash.
    Success {
        /// The total supply in motes.
        total_supply: U512,
    },
    /// Failed to get total supply.
    Failure(TrackingCopyError),
}
