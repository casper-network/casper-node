//! Contains implementation of the Auction contract functionality.
mod bid;
mod bid_addr;
mod bid_kind;
mod bridge;
mod constants;
mod delegator;
mod delegator_bid;
mod delegator_kind;
mod entry_points;
mod era_info;
mod error;
mod reservation;
mod seigniorage_recipient;
mod unbond;
mod unbonding_purse;
mod validator_bid;
mod validator_credit;
mod withdraw_purse;

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use alloc::collections::btree_map::Entry;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use itertools::Itertools;

use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};

pub use bid::{Bid, VESTING_SCHEDULE_LENGTH_MILLIS};
pub use bid_addr::{BidAddr, BidAddrTag};
pub use bid_kind::{BidKind, BidKindTag};
pub use bridge::Bridge;
pub use constants::*;
pub use delegator::Delegator;
pub use delegator_bid::DelegatorBid;
pub use delegator_kind::DelegatorKind;
pub use entry_points::auction_entry_points;
pub use era_info::{EraInfo, SeigniorageAllocation};
pub use error::Error;
pub use reservation::Reservation;
pub use seigniorage_recipient::{
    SeigniorageRecipient, SeigniorageRecipientV1, SeigniorageRecipientV2,
};
pub use unbond::{Unbond, UnbondEra, UnbondKind};
pub use unbonding_purse::UnbondingPurse;
pub use validator_bid::ValidatorBid;
pub use validator_credit::ValidatorCredit;
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

/// Delegator bids mapped to their validator.
pub type DelegatorBids = BTreeMap<PublicKey, Vec<Box<DelegatorBid>>>;

/// Reservations mapped to their validator.
pub type Reservations = BTreeMap<PublicKey, Vec<Box<Reservation>>>;

/// Validators mapped to their credits by era.
pub type ValidatorCredits = BTreeMap<PublicKey, BTreeMap<EraId, Box<ValidatorCredit>>>;

/// Weights of validators. "Weight" in this context means a sum of their stakes.
pub type ValidatorWeights = BTreeMap<PublicKey, U512>;

/// List of era validators
pub type EraValidators = BTreeMap<EraId, ValidatorWeights>;

/// Collection of seigniorage recipients. Legacy version.
pub type SeigniorageRecipientsV1 = BTreeMap<PublicKey, SeigniorageRecipientV1>;
/// Collection of seigniorage recipients.
pub type SeigniorageRecipientsV2 = BTreeMap<PublicKey, SeigniorageRecipientV2>;
/// Wrapper enum for all variants of `SeigniorageRecipients`.
#[allow(missing_docs)]
pub enum SeigniorageRecipients {
    V1(SeigniorageRecipientsV1),
    V2(SeigniorageRecipientsV2),
}

/// Snapshot of `SeigniorageRecipients` for a given era. Legacy version.
pub type SeigniorageRecipientsSnapshotV1 = BTreeMap<EraId, SeigniorageRecipientsV1>;
/// Snapshot of `SeigniorageRecipients` for a given era.
pub type SeigniorageRecipientsSnapshotV2 = BTreeMap<EraId, SeigniorageRecipientsV2>;
/// Wrapper enum for all variants of `SeigniorageRecipientsSnapshot`.
#[derive(Debug)]
#[allow(missing_docs)]
pub enum SeigniorageRecipientsSnapshot {
    V1(SeigniorageRecipientsSnapshotV1),
    V2(SeigniorageRecipientsSnapshotV2),
}

impl SeigniorageRecipientsSnapshot {
    /// Returns rewards for given validator in a specified era
    pub fn get_seignorage_recipient(
        &self,
        era_id: &EraId,
        validator_public_key: &PublicKey,
    ) -> Option<SeigniorageRecipient> {
        match self {
            Self::V1(snapshot) => snapshot.get(era_id).and_then(|era| {
                era.get(validator_public_key)
                    .map(|recipient| SeigniorageRecipient::V1(recipient.clone()))
            }),
            Self::V2(snapshot) => snapshot.get(era_id).and_then(|era| {
                era.get(validator_public_key)
                    .map(|recipient| SeigniorageRecipient::V2(recipient.clone()))
            }),
        }
    }
}

/// Validators and delegators mapped to their withdraw purses.
pub type WithdrawPurses = BTreeMap<AccountHash, Vec<WithdrawPurse>>;

/// Aggregated representation of validator and associated delegator bids.
pub type Staking = BTreeMap<PublicKey, (ValidatorBid, BTreeMap<DelegatorKind, DelegatorBid>)>;

/// Utils for working with a vector of BidKind.
#[cfg(any(all(feature = "std", feature = "testing"), test))]
pub trait BidsExt {
    /// Returns Bid matching public_key, if present.
    fn unified_bid(&self, public_key: &PublicKey) -> Option<Bid>;

    /// Returns ValidatorBid matching public_key, if present.
    fn validator_bid(&self, public_key: &PublicKey) -> Option<ValidatorBid>;

    /// Returns a bridge record matching old and new public key, if present.
    fn bridge(
        &self,
        public_key: &PublicKey,
        new_public_key: &PublicKey,
        era_id: &EraId,
    ) -> Option<Bridge>;

    /// Returns ValidatorCredit matching public_key, if present.
    fn credit(&self, public_key: &PublicKey) -> Option<ValidatorCredit>;

    /// Returns total validator stake, if present.
    fn validator_total_stake(&self, public_key: &PublicKey) -> Option<U512>;

    /// Returns Delegator entries matching validator public key, if present.
    fn delegators_by_validator_public_key(
        &self,
        public_key: &PublicKey,
    ) -> Option<Vec<DelegatorBid>>;

    /// Returns Delegator entry, if present.
    fn delegator_by_kind(
        &self,
        validator_public_key: &PublicKey,
        delegator_kind: &DelegatorKind,
    ) -> Option<DelegatorBid>;

    /// Returns Reservation entries matching validator public key, if present.
    fn reservations_by_validator_public_key(
        &self,
        public_key: &PublicKey,
    ) -> Option<Vec<Reservation>>;

    /// Returns Reservation entry, if present.
    fn reservation_by_kind(
        &self,
        validator_public_key: &PublicKey,
        delegator_kind: &DelegatorKind,
    ) -> Option<Reservation>;

    /// Returns Unbond entry, if present.
    fn unbond_by_kind(
        &self,
        validator_public_key: &PublicKey,
        unbond_kind: &UnbondKind,
    ) -> Option<Unbond>;

    /// Returns true if containing any elements matching the provided validator public key.
    fn contains_validator_public_key(&self, public_key: &PublicKey) -> bool;

    /// Removes any items with a public key matching the provided validator public key.
    fn remove_by_validator_public_key(&mut self, public_key: &PublicKey);

    /// Creates a map of Validator public keys to associated Delegators.
    fn delegator_map(&self) -> BTreeMap<PublicKey, Vec<DelegatorKind>>;

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

    fn bridge(
        &self,
        public_key: &PublicKey,
        new_public_key: &PublicKey,
        era_id: &EraId,
    ) -> Option<Bridge> {
        self.iter().find_map(|x| match x {
            BidKind::Bridge(bridge)
                if bridge.old_validator_public_key() == public_key
                    && bridge.new_validator_public_key() == new_public_key
                    && bridge.era_id() == era_id =>
            {
                Some(*bridge.clone())
            }
            _ => None,
        })
    }

    fn credit(&self, public_key: &PublicKey) -> Option<ValidatorCredit> {
        if let BidKind::Credit(credit) = self
            .iter()
            .find(|x| x.is_credit() && &x.validator_public_key() == public_key)?
        {
            Some(*credit.clone())
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

    fn delegators_by_validator_public_key(
        &self,
        public_key: &PublicKey,
    ) -> Option<Vec<DelegatorBid>> {
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

    fn delegator_by_kind(
        &self,
        validator_public_key: &PublicKey,
        delegator_kind: &DelegatorKind,
    ) -> Option<DelegatorBid> {
        if let BidKind::Delegator(delegator) = self.iter().find(|x| {
            x.is_delegator()
                && &x.validator_public_key() == validator_public_key
                && x.delegator_kind() == Some(delegator_kind.clone())
        })? {
            Some(*delegator.clone())
        } else {
            None
        }
    }

    fn reservations_by_validator_public_key(
        &self,
        validator_public_key: &PublicKey,
    ) -> Option<Vec<Reservation>> {
        let mut ret = vec![];
        for reservation in self
            .iter()
            .filter(|x| x.is_reservation() && &x.validator_public_key() == validator_public_key)
        {
            if let BidKind::Reservation(reservation) = reservation {
                ret.push(*reservation.clone());
            }
        }

        if ret.is_empty() {
            None
        } else {
            Some(ret)
        }
    }

    fn reservation_by_kind(
        &self,
        validator_public_key: &PublicKey,
        delegator_kind: &DelegatorKind,
    ) -> Option<Reservation> {
        if let BidKind::Reservation(reservation) = self.iter().find(|x| {
            x.is_reservation()
                && &x.validator_public_key() == validator_public_key
                && x.delegator_kind() == Some(delegator_kind.clone())
        })? {
            Some(*reservation.clone())
        } else {
            None
        }
    }

    fn unbond_by_kind(
        &self,
        validator_public_key: &PublicKey,
        unbond_kind: &UnbondKind,
    ) -> Option<Unbond> {
        if let BidKind::Unbond(unbond) = self.iter().find(|x| {
            x.is_unbond()
                && &x.validator_public_key() == validator_public_key
                && x.unbond_kind() == Some(unbond_kind.clone())
        })? {
            Some(*unbond.clone())
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

    fn delegator_map(&self) -> BTreeMap<PublicKey, Vec<DelegatorKind>> {
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
                        ve.insert(vec![delegator.delegator_kind().clone()]);
                    }
                    Entry::Occupied(mut oe) => {
                        let delegators = oe.get_mut();
                        delegators.push(delegator.delegator_kind().clone())
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
                    .map(|(_, y)| DelegatorKind::PublicKey(y.delegator_public_key().clone()))
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
                        && x.delegator_kind() == bid_kind.delegator_kind()
                })
                .map(|(idx, _)| idx),
            BidKind::Bridge(_) => self
                .iter()
                .find_position(|x| {
                    x.is_bridge()
                        && x.validator_public_key() == bid_kind.validator_public_key()
                        && x.new_validator_public_key() == bid_kind.new_validator_public_key()
                        && x.era_id() == bid_kind.era_id()
                })
                .map(|(idx, _)| idx),
            BidKind::Credit(_) => self
                .iter()
                .find_position(|x| {
                    x.validator_public_key() == bid_kind.validator_public_key()
                        && x.tag() == bid_kind.tag()
                        && x.era_id() == bid_kind.era_id()
                })
                .map(|(idx, _)| idx),
            BidKind::Reservation(_) => self
                .iter()
                .find_position(|x| {
                    x.is_reservation()
                        && x.validator_public_key() == bid_kind.validator_public_key()
                        && x.delegator_kind() == bid_kind.delegator_kind()
                })
                .map(|(idx, _)| idx),
            BidKind::Unbond(_) => self
                .iter()
                .find_position(|x| {
                    x.is_unbond()
                        && x.validator_public_key() == bid_kind.validator_public_key()
                        && x.unbond_kind() == bid_kind.unbond_kind()
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
mod prop_test_delegator {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid(bid in gens::delegator_arb()) {
            bytesrepr::test_serialization_roundtrip(&bid);
        }
    }
}

#[cfg(test)]
mod prop_test_reservation {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid(bid in gens::reservation_arb()) {
            bytesrepr::test_serialization_roundtrip(&bid);
        }
    }
}
