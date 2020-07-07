#![no_std]
#![no_main]

use contract::contract_api::runtime;
use types::{ApiError, U512};

const ARG_NUMBER: &str = "number";

#[no_mangle]
pub extern "C" fn call() {
    let number: U512 = runtime::get_named_arg(ARG_NUMBER);

    let user_code: u16 = number.as_u32() as u16;
    runtime::revert(ApiError::User(user_code));
}
