#![no_std]
#![no_main]

use casperlabs_contract::contract_api::runtime;
use casperlabs_types::ApiError;

#[no_mangle]
pub extern "C" fn call() {
    runtime::revert(ApiError::User(100))
}
