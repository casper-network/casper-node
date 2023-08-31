//! Contains implementation of a Auction contract functionality.
mod bid;
mod bid_addr;
mod bid_kind;
mod constants;
mod delegator;
mod entry_points;
mod era_info;
mod error;
mod seigniorage_recipient;
mod unbonding_purse;
mod validator_bid;
mod withdraw_purse;

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use alloc::collections::btree_map::Entry;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use itertools::Itertools;

use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};

pub use bid::{Bid, VESTING_SCHEDULE_LENGTH_MILLIS};
pub use bid_addr::{BidAddr, BidAddrTag};
pub use bid_kind::{BidKind, BidKindTag};
pub use constants::*;
pub use delegator::Delegator;
pub use entry_points::auction_entry_points;
pub use era_info::{EraInfo, SeigniorageAllocation};
pub use error::Error;
pub use seigniorage_recipient::SeigniorageRecipient;
pub use unbonding_purse::UnbondingPurse;
pub use validator_bid::ValidatorBid;
pub use withdraw_purse::WithdrawPurse;

#[cfg(any(feature = "testing", test))]
pub(crate) mod gens {
    pub use super::era_info::gens::*;
}

use crate::{account::AccountHash, EraId, PublicKey, U512};

/// Representation of delegation rate of tokens. Range from 0..=100.
pub type DelegationRate = u8;

/// Validators mapped to their bids.
pub type ValidatorBids = BTreeMap<PublicKey, Box<ValidatorBid>>;

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

/// Aggregated representation of validator and associated delegator bids.
pub type Staking = BTreeMap<PublicKey, (ValidatorBid, BTreeMap<PublicKey, Delegator>)>;

/// Utils for working with a vector of BidKind.
#[cfg(any(all(feature = "std", feature = "testing"), test))]
pub trait BidsExt {
    /// Returns Bid matching public_key, if present.
    fn unified_bid(&self, public_key: &PublicKey) -> Option<Bid>;

    /// Returns ValidatorBid matching public_key, if present.
    fn validator_bid(&self, public_key: &PublicKey) -> Option<ValidatorBid>;

    /// Returns total validator stake, if present.
    fn validator_total_stake(&self, public_key: &PublicKey) -> Option<U512>;

    /// Returns Delegator entries matching validator public key, if present.
    fn delegators_by_validator_public_key(&self, public_key: &PublicKey) -> Option<Vec<Delegator>>;

    /// Returns Delegator entry by public keys, if present.
    fn delegator_by_public_keys(
        &self,
        validator_public_key: &PublicKey,
        delegator_public_key: &PublicKey,
    ) -> Option<Delegator>;

    /// Returns true if containing any elements matching the provided validator public key.
    fn contains_validator_public_key(&self, public_key: &PublicKey) -> bool;

    /// Removes any items with a public key matching the provided validator public key.
    fn remove_by_validator_public_key(&mut self, public_key: &PublicKey);

    /// Creates a map of Validator public keys to associated Delegator public keys.
    fn public_key_map(&self) -> BTreeMap<PublicKey, Vec<PublicKey>>;

    /// Inserts if bid_kind does not exist, otherwise replaces.
    fn upsert(&mut self, bid_kind: BidKind);
}

#[cfg(any(all(feature = "std", feature = "testing"), test))]
impl BidsExt for Vec<BidKind> {
    fn unified_bid(&self, public_key: &PublicKey) -> Option<Bid> {
        if let BidKind::Unified(bid) = self
            .iter()
            .find(|x| x.is_validator() && &x.validator_public_key() == public_key)?
        {
            Some(*bid.clone())
        } else {
            None
        }
    }

    fn validator_bid(&self, public_key: &PublicKey) -> Option<ValidatorBid> {
        if let BidKind::Validator(validator_bid) = self
            .iter()
            .find(|x| x.is_validator() && &x.validator_public_key() == public_key)?
        {
            Some(*validator_bid.clone())
        } else {
            None
        }
    }

    fn validator_total_stake(&self, public_key: &PublicKey) -> Option<U512> {
        if let Some(validator_bid) = self.validator_bid(public_key) {
            let delegator_stake = {
                match self.delegators_by_validator_public_key(validator_bid.validator_public_key())
                {
                    None => U512::zero(),
                    Some(delegators) => delegators.iter().map(|x| x.staked_amount()).sum(),
                }
            };
            return Some(validator_bid.staked_amount() + delegator_stake);
        }

        if let BidKind::Unified(bid) = self
            .iter()
            .find(|x| x.is_validator() && &x.validator_public_key() == public_key)?
        {
            return Some(*bid.staked_amount());
        }

        None
    }

    fn delegators_by_validator_public_key(&self, public_key: &PublicKey) -> Option<Vec<Delegator>> {
        let mut ret = vec![];
        for delegator in self
            .iter()
            .filter(|x| x.is_delegator() && &x.validator_public_key() == public_key)
        {
            if let BidKind::Delegator(delegator) = delegator {
                ret.push(*delegator.clone());
            }
        }

        if ret.is_empty() {
            None
        } else {
            Some(ret)
        }
    }

    fn delegator_by_public_keys(
        &self,
        validator_public_key: &PublicKey,
        delegator_public_key: &PublicKey,
    ) -> Option<Delegator> {
        if let BidKind::Delegator(delegator) = self.iter().find(|x| {
            &x.validator_public_key() == validator_public_key
                && x.delegator_public_key() == Some(delegator_public_key.clone())
        })? {
            Some(*delegator.clone())
        } else {
            None
        }
    }

    fn contains_validator_public_key(&self, public_key: &PublicKey) -> bool {
        self.iter().any(|x| &x.validator_public_key() == public_key)
    }

    fn remove_by_validator_public_key(&mut self, public_key: &PublicKey) {
        self.retain(|x| &x.validator_public_key() != public_key)
    }

    fn public_key_map(&self) -> BTreeMap<PublicKey, Vec<PublicKey>> {
        let mut ret = BTreeMap::new();
        let validators = self
            .iter()
            .filter(|x| x.is_validator())
            .cloned()
            .collect_vec();
        for bid_kind in validators {
            ret.insert(bid_kind.validator_public_key().clone(), vec![]);
        }
        let delegators = self
            .iter()
            .filter(|x| x.is_delegator())
            .cloned()
            .collect_vec();
        for bid_kind in delegators {
            if let BidKind::Delegator(delegator) = bid_kind {
                match ret.entry(delegator.validator_public_key().clone()) {
                    Entry::Vacant(ve) => {
                        ve.insert(vec![delegator.delegator_public_key().clone()]);
                    }
                    Entry::Occupied(mut oe) => {
                        let delegators = oe.get_mut();
                        delegators.push(delegator.delegator_public_key().clone())
                    }
                }
            }
        }
        let unified = self
            .iter()
            .filter(|x| x.is_unified())
            .cloned()
            .collect_vec();
        for bid_kind in unified {
            if let BidKind::Unified(unified) = bid_kind {
                let delegators = unified
                    .delegators()
                    .iter()
                    .map(|(_, y)| y.delegator_public_key().clone())
                    .collect();
                ret.insert(unified.validator_public_key().clone(), delegators);
            }
        }
        ret
    }

    fn upsert(&mut self, bid_kind: BidKind) {
        let maybe_index = match bid_kind {
            BidKind::Unified(_) | BidKind::Validator(_) => self
                .iter()
                .find_position(|x| {
                    x.validator_public_key() == bid_kind.validator_public_key()
                        && x.tag() == bid_kind.tag()
                })
                .map(|(idx, _)| idx),
            BidKind::Delegator(_) => self
                .iter()
                .find_position(|x| {
                    x.is_delegator()
                        && x.validator_public_key() == bid_kind.validator_public_key()
                        && x.delegator_public_key() == bid_kind.delegator_public_key()
                })
                .map(|(idx, _)| idx),
        };

        match maybe_index {
            Some(index) => {
                self.insert(index, bid_kind);
            }
            None => {
                self.push(bid_kind);
            }
        }
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid(bid in gens::delegator_arb()) {
            bytesrepr::test_serialization_roundtrip(&bid);
        }
    }
}
