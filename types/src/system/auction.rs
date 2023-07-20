//! Contains implementation of a Auction contract functionality.
mod bid;
mod constants;
mod delegator;
mod entry_points;
mod era_info;
mod error;
mod seigniorage_recipient;
mod unbonding_purse;
mod withdraw_purse;

use alloc::{collections::BTreeMap, vec::Vec};

pub use bid::{Bid, VESTING_SCHEDULE_LENGTH_MILLIS};
pub use constants::*;
pub use delegator::Delegator;
pub use entry_points::auction_entry_points;
pub use era_info::{EraInfo, SeigniorageAllocation};
pub use error::Error;
pub use seigniorage_recipient::SeigniorageRecipient;
pub use unbonding_purse::UnbondingPurse;
pub use withdraw_purse::WithdrawPurse;

#[cfg(any(feature = "testing", test))]
pub(crate) mod gens {
    pub use super::era_info::gens::*;
}

use crate::{account::AccountHash, EraId, PublicKey, U512};

/// Representation of delegation rate of tokens. Range from 0..=100.
pub type DelegationRate = u8;

/// Validators mapped to their bids.
pub type Bids = BTreeMap<PublicKey, Bid>;

/// Weights of validators. "Weight" in this context means a sum of their stakes.
pub type ValidatorWeights = BTreeMap<PublicKey, U512>;

/// List of era validators
pub type EraValidators = BTreeMap<EraId, ValidatorWeights>;

/// Collection of seigniorage recipients.
pub type SeigniorageRecipients = BTreeMap<PublicKey, SeigniorageRecipient>;

/// Snapshot of `SeigniorageRecipients` for a given era.
pub type SeigniorageRecipientsSnapshot = BTreeMap<EraId, SeigniorageRecipients>;

/// Validators and delegators mapped to their unbonding purses.
pub type UnbondingPurses = BTreeMap<AccountHash, Vec<UnbondingPurse>>;

/// Validators and delegators mapped to their withdraw purses.
pub type WithdrawPurses = BTreeMap<AccountHash, Vec<WithdrawPurse>>;
