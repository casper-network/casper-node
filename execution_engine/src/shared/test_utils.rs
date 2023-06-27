//! Some functions to use in tests.

use casper_types::{
    contracts::{AccountHash, NamedKeys},
    AccessRights, CLValue, ContractHash, Key, StoredValue, URef,
};

/// Returns an account value paired with its key
pub fn mocked_account(account_hash: AccountHash) -> Vec<(Key, StoredValue)> {
    let contract_hash = ContractHash::default();
    vec![(
        Key::Account(account_hash),
        StoredValue::Account(
            CLValue::from_t(contract_hash).expect("must convert from contract hash"),
        ),
    )]
}
