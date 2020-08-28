//! Contains implementation of a Auction contract functionality.
mod args;
mod auction_provider;
mod bid;
mod delegator;
mod era_validators;
mod internal;
mod providers;
mod seigniorage_recipient;
mod types;
mod unbonding_purse;

pub use args::*;
pub use auction_provider::AuctionProvider;
pub use auction_provider::{
    BidPurses, BID_PURSES_KEY, DEFAULT_UNBONDING_DELAY, UNBONDING_PURSES_KEY,
};
pub use bid::{Bid, Bids};
pub use delegator::{DelegatedAmounts, Delegators};
pub use era_validators::{EraId, EraValidators, ValidatorWeights};
pub use providers::{RuntimeProvider, StorageProvider, SystemProvider};
pub use seigniorage_recipient::{
    SeigniorageRecipient, SeigniorageRecipients, SeigniorageRecipientsSnapshot,
};
pub use types::DelegationRate;
pub use unbonding_purse::{UnbondingPurse, UnbondingPurses};
