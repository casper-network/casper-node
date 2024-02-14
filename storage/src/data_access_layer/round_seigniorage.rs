use crate::tracking_copy::TrackingCopyError;
use casper_types::{Digest, U512};
use num_rational::Ratio;

/// Request to get the current round seigniorage rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundSeigniorageRateRequest {
    state_hash: Digest,
}

impl RoundSeigniorageRateRequest {
    /// Create instance of RoundSeigniorageRateRequest.
    pub fn new(state_hash: Digest) -> Self {
        RoundSeigniorageRateRequest { state_hash }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }
}

/// Represents a result of a `round_seigniorage_rate` request.
#[derive(Debug)]
pub enum RoundSeigniorageRateResult {
    /// Invalid state root hash.
    RootNotFound,
    /// The mint is not found.
    MintNotFound,
    /// Value not found.
    ValueNotFound(String),
    /// The round seigniorage rate at the specified state hash.
    Success {
        /// The current rate.
        rate: Ratio<U512>,
    },
    Failure(TrackingCopyError),
}
