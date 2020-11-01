use serde::{Deserialize, Serialize};

use crate::crypto::hash::Digest;
use casper_types::auction::{Bids, EraId, ValidatorWeights};

/// Data structure summarizing auction contract data.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct AuctionState {
    /// Global state hash
    state_root_hash: Digest,
    /// Era id.
    era_id: EraId,
    /// All bids.
    bids: Option<Bids>,
    /// Validator weights for this era.
    validator_weights: Option<ValidatorWeights>,
}

impl AuctionState {
    /// Create new instance of `AuctionState`
    pub fn new(
        state_root_hash: Digest,
        era_id: EraId,
        bids: Option<Bids>,
        validator_weights: Option<ValidatorWeights>,
    ) -> Self {
        AuctionState {
            state_root_hash,
            era_id,
            bids,
            validator_weights,
        }
    }
}
