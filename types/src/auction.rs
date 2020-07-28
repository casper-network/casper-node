//! Contains implementation of a Auction contract functionality.
mod active_bid;
mod founding_validator;
mod seigniorage_recipient;
mod types;

pub use active_bid::{ActiveBid, ActiveBids};
pub use founding_validator::{FoundingValidator, FoundingValidators};
pub use seigniorage_recipient::{SeigniorageRecipient, SeigniorageRecipients};
pub use types::DelegationRate;
