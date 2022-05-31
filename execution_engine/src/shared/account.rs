//! Execution-related account creation helpers
//!
//! This module uses [`casper_types::account::Account`] and adds extra logic
//! specific to a public, or private chains.
//!
//! Production code should always use this module to create new account instances before writing
//! them to a global state.
use casper_types::{
    account::{Account, AccountHash},
    contracts::NamedKeys,
    PublicKey, URef,
};
use once_cell::sync::Lazy;

pub(crate) static SYSTEM_ACCOUNT_ADDRESS: Lazy<AccountHash> =
    Lazy::new(|| PublicKey::System.to_account_hash());

/// Creates new account.
pub fn create_account(account_hash: AccountHash, main_purse: URef) -> Account {
    let named_keys = NamedKeys::default();
    Account::create(account_hash, named_keys, main_purse)
}
