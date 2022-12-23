use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::HashSet;

#[cfg(test)]
use casper_types::{
    account::{Account, AccountHash},
    system::auction::Bid,
    CLValue, PublicKey, URef, U512,
};
use casper_types::{Key, StoredValue};

#[cfg(test)]
use super::state_reader::StateReader;

use crate::utils::{print_entry, print_validators, ValidatorInfo};

pub(crate) struct Update {
    entries: BTreeMap<Key, StoredValue>,
    // Holds the complete set of validators, only if the validator set changed
    validators: Option<Vec<ValidatorInfo>>,
}

impl Update {
    pub(crate) fn new(
        entries: BTreeMap<Key, StoredValue>,
        validators: Option<Vec<ValidatorInfo>>,
    ) -> Self {
        Self {
            entries,
            validators,
        }
    }

    pub(crate) fn print(&self) {
        if let Some(validators) = &self.validators {
            print_validators(validators);
        }
        for (key, value) in &self.entries {
            print_entry(key, value);
        }
    }
}

#[cfg(test)]
impl Update {
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn get_written_account(&self, account: AccountHash) -> Account {
        self.entries
            .get(&Key::Account(account))
            .expect("account should exist")
            .as_account()
            .expect("should be an account")
            .clone()
    }

    pub(crate) fn get_written_bid(&self, account: AccountHash) -> Bid {
        self.entries
            .get(&Key::Bid(account))
            .expect("should create bid")
            .as_bid()
            .expect("should be bid")
            .clone()
    }

    pub(crate) fn assert_written_balance(&self, purse: URef, balance: u64) {
        assert_eq!(
            self.entries.get(&Key::Balance(purse.addr())),
            Some(&StoredValue::from(
                CLValue::from_t(U512::from(balance)).expect("should convert U512 to CLValue")
            ))
        );
    }

    pub(crate) fn assert_total_supply<R: StateReader>(&self, reader: &mut R, supply: u64) {
        assert_eq!(
            self.entries.get(&reader.get_total_supply_key()),
            Some(&StoredValue::from(
                CLValue::from_t(U512::from(supply)).expect("should convert U512 to CLValue")
            ))
        );
    }

    pub(crate) fn assert_written_purse_is_unit(&self, purse: URef) {
        assert_eq!(
            self.entries.get(&Key::URef(purse)),
            Some(&StoredValue::from(
                CLValue::from_t(()).expect("should convert unit to CLValue")
            ))
        );
    }

    pub(crate) fn assert_seigniorage_recipients_written<R: StateReader>(&self, reader: &mut R) {
        assert!(self
            .entries
            .contains_key(&reader.get_seigniorage_recipients_key()));
    }

    pub(crate) fn assert_written_bid(&self, account: AccountHash, bid: Bid) {
        assert_eq!(
            self.entries.get(&Key::Bid(account)),
            Some(&StoredValue::from(bid))
        );
    }

    pub(crate) fn assert_unbonding_purse(
        &self,
        bid_purse: URef,
        validator_key: &PublicKey,
        unbonder_key: &PublicKey,
        amount: u64,
    ) {
        let account_hash = validator_key.to_account_hash();
        let unbonds = self
            .entries
            .get(&Key::Unbond(account_hash))
            .expect("should have unbonds for the account")
            .as_unbonding()
            .expect("should be unbonding purses");
        assert!(unbonds.iter().any(
            |unbonding_purse| unbonding_purse.bonding_purse() == &bid_purse
                && unbonding_purse.validator_public_key() == validator_key
                && unbonding_purse.unbonder_public_key() == unbonder_key
                && unbonding_purse.amount() == &U512::from(amount)
        ))
    }

    pub(crate) fn assert_key_absent(&self, key: &Key) {
        assert!(!self.entries.contains_key(key))
    }

    pub(crate) fn assert_validators(&self, validators: &[ValidatorInfo]) {
        let self_set: HashSet<_> = self.validators.as_ref().unwrap().iter().collect();
        let other_set: HashSet<_> = validators.iter().collect();
        assert_eq!(self_set, other_set);
    }

    pub(crate) fn assert_validators_unchanged(&self) {
        assert!(self.validators.is_none());
    }
}
