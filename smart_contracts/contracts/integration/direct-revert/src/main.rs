#![no_std]
#![no_main]

use contract::contract_api::runtime;
use types::ApiError;

#[no_mangle]
pub extern "C" fn call() {
    // Call revert with an application specific non-zero exit code.
    runtime::revert(ApiError::User(1));
}
