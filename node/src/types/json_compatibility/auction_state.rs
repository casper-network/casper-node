use serde::{Deserialize, Serialize};

use casper_types::auction::{Bids, EraValidators};

use crate::crypto::hash::Digest;

/// Data structure summarizing auction contract data.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct AuctionState {
    /// Global state hash
    pub state_root_hash: Digest,
    /// Block height
    pub block_height: u64,
    /// Era validators
    pub era_validators: Option<EraValidators>,
    /// All bids.
    bids: Option<Bids>,
}

impl AuctionState {
    /// Create new instance of `AuctionState`
    pub fn new(
        state_root_hash: Digest,
        block_height: u64,
        era_validators: Option<EraValidators>,
        bids: Option<Bids>,
    ) -> Self {
        AuctionState {
            state_root_hash,
            block_height,
            era_validators,
            bids,
        }
    }
}
