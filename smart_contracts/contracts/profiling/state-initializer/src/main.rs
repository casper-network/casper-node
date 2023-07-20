//! Transfers the requested amount of motes to the first account and zero motes to the second
//! account.
#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, system};
use casper_types::{account::AccountHash, ApiError, TransferredTo, U512};

const ARG_ACCOUNT1_ACCOUNT_HASH: &str = "account_1_account_hash";
const ARG_ACCOUNT1_AMOUNT: &str = "account_1_amount";
const ARG_ACCOUNT2_ACCOUNT_HASH: &str = "account_2_account_hash";

#[repr(u16)]
enum Error {
    AccountAlreadyExists = 0,
}

fn create_account_with_amount(account: AccountHash, amount: U512) {
    match system::transfer_to_account(account, amount, None) {
        Ok(TransferredTo::NewAccount) => (),
        Ok(TransferredTo::ExistingAccount) => {
            runtime::revert(ApiError::User(Error::AccountAlreadyExists as u16))
        }
        Err(_) => runtime::revert(ApiError::Transfer),
    }
}

#[no_mangle]
pub extern "C" fn call() {
    let account_hash1: AccountHash = runtime::get_named_arg(ARG_ACCOUNT1_ACCOUNT_HASH);
    let amount: U512 = runtime::get_named_arg(ARG_ACCOUNT1_AMOUNT);
    create_account_with_amount(account_hash1, amount);

    let account_hash2: AccountHash = runtime::get_named_arg(ARG_ACCOUNT2_ACCOUNT_HASH);
    create_account_with_amount(account_hash2, U512::zero());
}
