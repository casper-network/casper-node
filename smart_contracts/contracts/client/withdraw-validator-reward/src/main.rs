#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{account, runtime, system};
use casper_types::{auction, runtime_args, PublicKey, RuntimeArgs, URef};

fn withdraw_validator_reward(public_key: PublicKey, target_purse: URef) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_VALIDATOR_PUBLIC_KEY => public_key,
        auction::ARG_TARGET_PURSE => target_purse
    };
    runtime::call_contract(
        contract_hash,
        auction::METHOD_WITHDRAW_VALIDATOR_REWARD,
        args,
    )
}

#[no_mangle]
pub extern "C" fn call() {
    let public_key: PublicKey = runtime::get_named_arg(auction::ARG_VALIDATOR_PUBLIC_KEY);

    let target_purse: URef = {
        let maybe_target_purse: Option<URef> = runtime::get_named_arg(auction::ARG_TARGET_PURSE);
        maybe_target_purse.unwrap_or_else(account::get_main_purse)
    };

    withdraw_validator_reward(public_key, target_purse);
}
