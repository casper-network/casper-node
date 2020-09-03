#![no_std]
#![no_main]

use casper_contract::contract_api::runtime;
use casper_types::ApiError;

#[no_mangle]
pub extern "C" fn call() {
    // Call revert with an application specific non-zero exit code.
    runtime::revert(ApiError::User(1));
}
