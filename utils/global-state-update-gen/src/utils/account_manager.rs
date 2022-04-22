use std::collections::BTreeMap;

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
        // unfortunately, `if let Some(account) = self.accounts.get(&public_key)` upsets the borrow
        // checker, so we have to do it this way
        if !self.accounts.contains_key(public_key) {
            let account_hash = public_key.to_account_hash();
            if let Some(account) = builder.get_account(account_hash) {
                let _ = self.accounts.insert(public_key.clone(), account);
                // unwrapping is safe, we just inserted it
                self.accounts.get(public_key).unwrap()
            } else {
                self.create_account(public_key.clone());
                // create_account inserts the account, so unwrapping is safe
                self.accounts.get(public_key).unwrap()
            }
        } else {
            // we know it contains the key, unwrapping safe
            self.accounts.get(public_key).unwrap()
        }
    }

    fn create_account(&mut self, public_key: PublicKey) {
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
        let _ = self.accounts.insert(public_key, account);
    }
}
