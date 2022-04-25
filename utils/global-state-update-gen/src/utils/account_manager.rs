use std::collections::{btree_map::Entry, BTreeMap};

use casper_engine_test_support::LmdbWasmTestBuilder;
use casper_types::{
    account::Account, AccessRights, CLValue, Key, PublicKey, StoredValue, URef, U512,
};

use rand::prelude::*;

use crate::utils::print_entry;

pub struct AccountManager {
    accounts: BTreeMap<PublicKey, Account>,
}

impl AccountManager {
    /// Creates a new instance of `AccountManager`.
    pub fn new() -> Self {
        Self {
            accounts: Default::default(),
        }
    }

    /// Gets the account for the given public key.
    /// If the account doesn't exist, it prints the entries necessary to create it and returns the
    /// created account.
    pub fn get_or_create_account(
        &mut self,
        builder: &mut LmdbWasmTestBuilder,
        public_key: &PublicKey,
    ) -> &Account {
        match self.accounts.entry(public_key.clone()) {
            Entry::Vacant(vac) => {
                let account_hash = public_key.to_account_hash();
                let account = match builder.get_account(account_hash) {
                    Some(account) => account,
                    None => Self::create_account(public_key.clone()),
                };
                vac.insert(account)
            }
            Entry::Occupied(occupied) => occupied.into_mut(),
        }
    }

    fn create_account(public_key: PublicKey) -> Account {
        let mut rng = rand::thread_rng();
        let new_purse = URef::new(rng.gen(), AccessRights::READ_ADD_WRITE);

        // Purse URef pointing to `()` so that the owner cannot modify the purse directly.
        print_entry(
            &Key::URef(new_purse),
            &StoredValue::CLValue(CLValue::unit()),
        );

        // This will create a new purse by storing a zero balance in it.
        print_entry(
            &Key::Balance(new_purse.addr()),
            &StoredValue::CLValue(CLValue::from_t(U512::zero()).unwrap()),
        );

        let account_hash = public_key.to_account_hash();
        let account = Account::create(account_hash, Default::default(), new_purse);
        // Store the account.
        print_entry(
            &Key::Account(account_hash),
            &StoredValue::Account(account.clone()),
        );

        account
    }
}
