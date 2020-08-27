//! Contains implementation of a Auction contract functionality.
mod active_bid;
mod args;
mod auction_provider;
mod delegator;
mod era_validators;
mod founding_validator;
mod internal;
mod providers;
mod seigniorage_recipient;
mod types;
mod unbonding_purse;

pub use active_bid::{ActiveBid, ActiveBids};
pub use args::*;
pub use auction_provider::AuctionProvider;
pub use auction_provider::{
    BidPurses, BID_PURSES_KEY, DEFAULT_UNBONDING_DELAY, UNBONDING_PURSES_KEY,
};
pub use delegator::{DelegatedAmounts, Delegators};
pub use era_validators::{EraId, EraValidators, ValidatorWeights};
pub use founding_validator::{FoundingValidator, FoundingValidators};
pub use providers::{RuntimeProvider, StorageProvider, SystemProvider};
pub use seigniorage_recipient::{
    SeigniorageRecipient, SeigniorageRecipients, SeigniorageRecipientsSnapshot,
};
pub use types::DelegationRate;
pub use unbonding_purse::{UnbondingPurse, UnbondingPurses};
