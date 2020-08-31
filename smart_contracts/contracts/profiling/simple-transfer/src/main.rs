#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{account::AccountHash, ApiError, TransferredTo, U512};

const ARG_ACCOUNT_HASH: &str = "account_hash";
const ARG_AMOUNT: &str = "amount";

#[repr(u16)]
enum Error {
    NonExistentAccount = 0,
}

#[no_mangle]
pub extern "C" fn call() {
    let account_hash: AccountHash = runtime::get_named_arg(ARG_ACCOUNT_HASH);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    match system::transfer_to_account(account_hash, amount).unwrap_or_revert() {
        TransferredTo::NewAccount => {
            runtime::revert(ApiError::User(Error::NonExistentAccount as u16))
        }
        TransferredTo::ExistingAccount => (),
    }
}
