#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash, system::handle_payment, ApiError, RuntimeArgs, URef, U512,
};

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

fn get_rewards_purse() -> URef {
    let handle_payment_hash = system::get_handle_payment();
    runtime::call_contract(
        handle_payment_hash,
        handle_payment::METHOD_GET_REWARDS_PURSE,
        RuntimeArgs::default(),
    )
}

#[no_mangle]
pub extern "C" fn call() {
    let source = get_rewards_purse();
    let target: AccountHash = runtime::get_named_arg(ARG_TARGET);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    match system::get_purse_balance(source) {
        Some(total_rewards) if total_rewards >= amount => {
            system::transfer_from_purse_to_account(source, target, amount, None).unwrap_or_revert();
        }
        Some(_total_rewards) => runtime::revert(ApiError::User(0)),
        None => runtime::revert(ApiError::User(1)),
    }
}
