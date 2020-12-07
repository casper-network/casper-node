#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, system};
use casper_types::{account::AccountHash, system_contract_errors::mint, ApiError, U512};

const ARG_AMOUNT: &str = "amount";

#[no_mangle]
pub extern "C" fn call() {
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let account_hash = AccountHash::new([42; 32]);
    let result = system::transfer_to_account(account_hash, amount, None);
    let expected_error: ApiError = mint::Error::InsufficientFunds.into();
    assert_eq!(result, Err(expected_error))
}
