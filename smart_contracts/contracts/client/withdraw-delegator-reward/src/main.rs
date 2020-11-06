#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{account, runtime, system};
use casper_types::{auction, runtime_args, PublicKey, RuntimeArgs, URef};

fn withdraw_delegator_reward(
    delegator_public_key: PublicKey,
    validator_public_key: PublicKey,
    target_purse: URef,
) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_DELEGATOR_PUBLIC_KEY => delegator_public_key,
        auction::ARG_VALIDATOR_PUBLIC_KEY => validator_public_key,
        auction::ARG_TARGET_PURSE => target_purse
    };
    runtime::call_contract(
        contract_hash,
        auction::METHOD_WITHDRAW_DELEGATOR_REWARD,
        args,
    )
}

#[no_mangle]
pub extern "C" fn call() {
    let delegator_public_key: PublicKey = runtime::get_named_arg(auction::ARG_DELEGATOR_PUBLIC_KEY);
    let validator_public_key: PublicKey = runtime::get_named_arg(auction::ARG_VALIDATOR_PUBLIC_KEY);

    let target_purse: URef = {
        let maybe_target_purse: Option<URef> = runtime::get_named_arg(auction::ARG_TARGET_PURSE);
        maybe_target_purse.unwrap_or_else(account::get_main_purse)
    };

    withdraw_delegator_reward(delegator_public_key, validator_public_key, target_purse);
}
