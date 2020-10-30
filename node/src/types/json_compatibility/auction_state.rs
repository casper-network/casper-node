use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{crypto::hash::Digest, types::json_compatibility};
use casper_types::{
    auction::{Bid as AuctionBid, Bids as AuctionBids, EraValidators as AuctionEraValidators},
    U512,
};

/// Bids table.
pub type Bids = BTreeMap<json_compatibility::PublicKey, Bid>;
/// Validator weights by validator key.
pub type ValidatorWeights = BTreeMap<json_compatibility::PublicKey, U512>;
/// List of era validators
pub type EraValidators = BTreeMap<u64, ValidatorWeights>;

/// An entry in a founding validator map.
#[derive(PartialEq, Debug, Deserialize, Serialize, Clone)]
pub struct Bid {
    /// The purse that was used for bonding.
    pub bonding_purse: String,
    /// The total amount of staked tokens.
    pub staked_amount: U512,
    /// Delegation rate.
    pub delegation_rate: u64,
    /// A flag that represents a winning entry.
    ///
    /// `Some` indicates locked funds for a specific era and an autowin status, and `None` case
    /// means that funds are unlocked and autowin status is removed.
    pub funds_locked: Option<u64>,
}

impl From<AuctionBid> for Bid {
    fn from(bid: AuctionBid) -> Self {
        Bid {
            bonding_purse: bid.bonding_purse.to_formatted_string(),
            staked_amount: bid.staked_amount,
            delegation_rate: bid.delegation_rate,
            funds_locked: bid.funds_locked,
        }
    }
}

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
    pub bids: Option<Bids>,
}

impl AuctionState {
    /// Create new instance of `AuctionState`
    pub fn new(
        state_root_hash: Digest,
        block_height: u64,
        bids: Option<AuctionBids>,
        era_validators: Option<AuctionEraValidators>,
    ) -> Self {
        let bids = bids.map(|items| {
            items
                .into_iter()
                .map(|(public_key, bid)| (public_key.into(), bid.into()))
                .collect()
        });

        let era_validators = era_validators.map(|items| {
            items
                .into_iter()
                .map(|(era_id, validator_weights)| {
                    (
                        era_id,
                        validator_weights
                            .into_iter()
                            .map(|(public_key, weight)| (public_key.into(), weight))
                            .collect(),
                    )
                })
                .collect()
        });

        AuctionState {
            state_root_hash,
            block_height,
            bids,
            era_validators,
        }
    }
}
