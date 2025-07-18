//! Support for obtaining current bids from the auction system.
use crate::tracking_copy::TrackingCopyError;

use casper_types::{
    system::auction::{BidAddr, BidKind, DelegatorKind},
    Digest, Key, PublicKey,
};

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
    /// Failure.
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

/// Represents a request to obtain validator bid with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorBidRequest {
    /// State identifier    
    state_root_hash: Digest,
    /// Public key identifying the validator
    validator_key: PublicKey,
}

impl ValidatorBidRequest {
    /// Creates new request.
    pub fn new(state_root_hash: Digest, validator_key: PublicKey) -> Self {
        Self {
            state_root_hash,
            validator_key,
        }
    }

    /// Returns state root hash.
    pub fn state_root_hash(&self) -> Digest {
        self.state_root_hash
    }

    /// Returns public key
    pub fn validator_key(&self) -> &PublicKey {
        &self.validator_key
    }
}

/// Represents a result of a `get_validator_bid` request.
#[derive(Debug, Eq, PartialEq)]
pub enum ValidatorBidsResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Contains data conforming to the request
    Success {
        /// Bids related to the validator
        bids: Vec<BidKind>,
    },
    /// Failure.
    Failure(TrackingCopyError),
}

/// Represents a request to obtain validator bid with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegatorBidRequest {
    /// State identifier
    state_root_hash: Digest,
    /// Public key identifying the validator   
    validator_public_key: PublicKey,
    /// Identifier pointing to a delegator in scope of the given validator
    delegator: DelegatorKind,
}

impl DelegatorBidRequest {
    /// Creates new request.
    pub fn new(
        state_root_hash: Digest,
        validator_key: PublicKey,
        delegator: DelegatorKind,
    ) -> Self {
        Self {
            state_root_hash,
            validator_public_key: validator_key,
            delegator,
        }
    }

    /// Returns state root hash.
    pub fn state_root_hash(&self) -> Digest {
        self.state_root_hash
    }

    /// Creates a Key adequate to fetch the validator of this request
    pub fn to_validator_key(&self) -> Key {
        let account_hash = self.validator_public_key.to_account_hash();
        Key::BidAddr(BidAddr::Validator(account_hash))
    }

    /// Creates a Key adequate to fetch the delegator of this request
    pub fn to_delegator_key(self) -> Key {
        let validator = self.validator_public_key.to_account_hash();
        match self.delegator {
            DelegatorKind::PublicKey(delegator) => Key::BidAddr(BidAddr::DelegatedAccount {
                validator,
                delegator: delegator.to_account_hash(),
            }),
            DelegatorKind::Purse(delegator) => Key::BidAddr(BidAddr::DelegatedPurse {
                validator,
                delegator,
            }),
        }
    }

    /// Validator public key
    pub fn validator_key(&self) -> &PublicKey {
        &self.validator_public_key
    }

    /// Delegator
    pub fn delegator(&self) -> &DelegatorKind {
        &self.delegator
    }
}

/// Represents a result of a `get_delegator_bid` request.
#[derive(Debug)]
pub enum DelegatorBidsResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Contains data conforming to the request
    Success {
        /// bids related with the delegator request
        bids: Vec<BidKind>,
    },
    /// Failure.
    Failure(TrackingCopyError),
}
