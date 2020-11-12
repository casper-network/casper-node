use std::collections::BTreeMap;

use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};

use crate::{
    crypto::{
        asymmetric_key::{PublicKey, SecretKey},
        hash::Digest,
    },
    rpcs::docs::DocExample,
    types::json_compatibility,
};
use casper_types::{
    auction::{Bid as AuctionBid, Bids as AuctionBids, EraValidators as AuctionEraValidators},
    bytesrepr::{FromBytes, ToBytes},
    U512,
};

use crate::crypto::hash::Digest;

/// Data structure summarizing auction contract data.
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
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

lazy_static! {
    static ref ERA_VALIDATORS: EraValidators = {
        let secret_key_1 = SecretKey::doc_example();
        let public_key_1 = PublicKey::from(secret_key_1);
        let asm_bytes = public_key_1.to_bytes().unwrap();
        let (casper_key, _) = casper_types::PublicKey::from_bytes(&asm_bytes).unwrap();
        let json_key = json_compatibility::PublicKey::from(casper_key);

        let mut validator_weights = BTreeMap::new();
        validator_weights.insert(json_key, U512::from(10));

        let mut era_validators = BTreeMap::new();
        era_validators.insert(10, validator_weights);

        era_validators
    };
    static ref BIDS: Bids = {
        let bonding_purse = String::from(
            "uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-007",
        );
        let staked_amount = U512::from(10);
        let delegation_rate = 10;
        let release_era = Some(43);

        let bid = Bid {
            bonding_purse,
            staked_amount,
            delegation_rate,
            release_era,
        };

        let secret_key_1 = SecretKey::doc_example();
        let public_key_1 = PublicKey::from(secret_key_1);
        let asm_bytes = public_key_1.to_bytes().unwrap();
        let (casper_key, _) = casper_types::PublicKey::from_bytes(&asm_bytes).unwrap();
        let json_key = json_compatibility::PublicKey::from(casper_key);

        let mut bids = BTreeMap::new();
        bids.insert(json_key, bid);

        bids
    };
    static ref AUCTION_INFO: AuctionState = {
        let state_root_hash = Digest::doc_example();
        let height: u64 = 10;
        let era_validators = Some(EraValidators::doc_example().clone());
        let bids = Some(Bids::doc_example().clone());
        AuctionState {
            state_root_hash,
            block_height: height,
            era_validators,
            bids,
        }
    };
}

impl DocExample for AuctionState {
    fn doc_example() -> &'static Self {
        &*AUCTION_INFO
    }
}

impl DocExample for EraValidators {
    fn doc_example() -> &'static Self {
        &*ERA_VALIDATORS
    }
}

impl DocExample for Bids {
    fn doc_example() -> &'static Self {
        &*BIDS
    }
}
