#![no_std]

extern crate alloc;

use alloc::format;

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{account::AccountHash, ApiError, Key, URef, U512};

const TRANSFER_RESULT_UREF_NAME: &str = "transfer_result";
const MAIN_PURSE_FINAL_BALANCE_UREF_NAME: &str = "final_balance";

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";
const ARG_ID: &str = "id";

pub fn delegate() {
    let source: URef = account::get_main_purse();
    let target: AccountHash = runtime::get_named_arg(ARG_TARGET);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let id: Option<u64> = runtime::get_named_arg(ARG_ID);

    let transfer_result = system::transfer_from_purse_to_account(source, target, amount, id);

    let final_balance =
        system::get_purse_balance(source).unwrap_or_revert_with(ApiError::User(103));

    let result = format!("{:?}", transfer_result);

    let result_uref: Key = storage::new_uref(result).into();
    runtime::put_key(TRANSFER_RESULT_UREF_NAME, result_uref);
    runtime::put_key(
        MAIN_PURSE_FINAL_BALANCE_UREF_NAME,
        storage::new_uref(final_balance).into(),
    );
}
