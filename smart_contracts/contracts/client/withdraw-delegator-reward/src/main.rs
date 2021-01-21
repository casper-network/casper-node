#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{auction, runtime_args, PublicKey, RuntimeArgs};

fn withdraw_delegator_reward(delegator_public_key: PublicKey, validator_public_key: PublicKey) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_DELEGATOR_PUBLIC_KEY => delegator_public_key,
        auction::ARG_VALIDATOR_PUBLIC_KEY => validator_public_key,
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

    withdraw_delegator_reward(delegator_public_key, validator_public_key);
}
