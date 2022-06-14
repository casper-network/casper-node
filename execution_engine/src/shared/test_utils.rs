//! Some functions to use in tests.

use casper_types::{
    account::{Account, AccountHash},
    contracts::NamedKeys,
    AccessRights, Key, StoredValue, URef,
};

/// Returns an account value paired with its key
pub fn mocked_account(account_hash: AccountHash) -> Vec<(Key, StoredValue)> {
    let purse = URef::new([0u8; 32], AccessRights::READ_ADD_WRITE);
    let account = Account::create(account_hash, NamedKeys::default(), purse);
    vec![(Key::Account(account_hash), StoredValue::Account(account))]
}
