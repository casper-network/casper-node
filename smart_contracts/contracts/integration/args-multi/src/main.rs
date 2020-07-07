#![no_std]
#![no_main]

use contract::contract_api::runtime;
use types::ApiError;

const ARG_ACCOUNT: &str = "account";
const ARG_NUMBER: &str = "number";

#[no_mangle]
pub extern "C" fn call() {
    let account_number: [u8; 32] = runtime::get_named_arg(ARG_ACCOUNT);
    let number: u32 = runtime::get_named_arg(ARG_NUMBER);

    let account_sum: u8 = account_number.iter().sum();
    let total_sum: u32 = u32::from(account_sum) + number;

    runtime::revert(ApiError::User(total_sum as u16));
}
