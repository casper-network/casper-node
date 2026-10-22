#![no_main]
#![no_std]

use casper_contract::contract_api::runtime;
use casper_types::account::AccountHash;
use casper_types::ApiError;

pub const ACCOUNT_HASH_ARG: &str = "account-hash";

#[no_mangle]
pub extern "C" fn call() {
    let account_hash = runtime::get_named_arg::<AccountHash>(ACCOUNT_HASH_ARG);
    let associated_keys = runtime::read_associated_keys(account_hash);
    // for the sake of this test, assume a default account with just one associated key
    if associated_keys.is_empty() {
        runtime::revert(ApiError::User(16u16))
    }
    if !associated_keys.contains_key(&account_hash) {
        runtime::revert(ApiError::User(17u16))
    }
}
