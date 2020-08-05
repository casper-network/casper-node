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

pub use active_bid::{ActiveBid, ActiveBids};
pub use args::*;
pub use auction_provider::AuctionProvider;
pub use era_validators::{EraId, EraValidators, ValidatorWeights};
pub use founding_validator::{FoundingValidator, FoundingValidators};
pub use providers::{MintProvider, RuntimeProvider, StorageProvider, SystemProvider};
pub use seigniorage_recipient::{
    SeigniorageRecipient, SeigniorageRecipients, SeigniorageRecipientsSnapshot,
};
pub use delegator::{Delegations, DelegationsMap};
pub use types::{CommissionRate, COMMISSION_RATE_DENOMINATOR};
