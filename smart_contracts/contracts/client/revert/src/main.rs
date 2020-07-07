#![no_std]
#![no_main]

use contract::contract_api::runtime;
use types::ApiError;

#[no_mangle]
pub extern "C" fn call() {
    runtime::revert(ApiError::User(100))
}
