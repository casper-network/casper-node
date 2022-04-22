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
        pkey: &PublicKey,
    ) -> &Account {
        // unfortunately, `if let Some(account) = self.accounts.get(&pkey)` upsets the borrow
        // checker, so we have to do it this way
        if !self.accounts.contains_key(pkey) {
            let account_hash = pkey.to_account_hash();
            if let Some(account) = builder.get_account(account_hash) {
                let _ = self.accounts.insert(pkey.clone(), account);
                // unwrapping is safe, we just inserted it
                self.accounts.get(pkey).unwrap()
            } else {
                self.create_account(pkey.clone());
                // create_account inserts the account, so unwrapping is safe
                self.accounts.get(pkey).unwrap()
            }
        } else {
            // we know it contains the key, unwrapping safe
            self.accounts.get(pkey).unwrap()
        }
    }

    fn create_account(&mut self, pkey: PublicKey) {
        let mut rng = rand::thread_rng();
        let new_purse = URef::new(rng.gen(), AccessRights::READ_ADD_WRITE);

        // Apparently EE does this, not sure why, but we'll also do it for consistency.
        print_entry(
            &Key::URef(new_purse),
            &StoredValue::CLValue(CLValue::unit()),
        );

        // This will create a new purse by storing a zero balance in it.
        print_entry(
            &Key::Balance(new_purse.addr()),
            &StoredValue::CLValue(CLValue::from_t(U512::zero()).unwrap()),
        );

        let account_hash = pkey.to_account_hash();
        let account = Account::create(account_hash, Default::default(), new_purse);
        // Store the account.
        print_entry(
            &Key::Account(account_hash),
            &StoredValue::Account(account.clone()),
        );
        let _ = self.accounts.insert(pkey, account);
    }
}
