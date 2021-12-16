#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeSet;

use casper_contract::contract_api::runtime;
use casper_types::{account::AccountHash, ApiError};

const ARG_EXPECTED_AUTHORIZATION_KEYS: &str = "expected_authorization_keys";

#[repr(u16)]
enum UserError {
    AssertionFail = 0,
}

impl From<UserError> for ApiError {
    fn from(error: UserError) -> ApiError {
        ApiError::User(error as u16)
    }
}

#[no_mangle]
pub extern "C" fn call() {
    let expected_authorized_keys: BTreeSet<AccountHash> =
        runtime::get_named_arg(ARG_EXPECTED_AUTHORIZATION_KEYS);

    let actual_authorized_keys = runtime::list_authorization_keys();

    if expected_authorized_keys != actual_authorized_keys {
        runtime::revert(UserError::AssertionFail)
    }
}
