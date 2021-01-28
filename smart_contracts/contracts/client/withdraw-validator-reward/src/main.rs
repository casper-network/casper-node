#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{auction, runtime_args, PublicKey, RuntimeArgs};

fn withdraw_validator_reward(public_key: PublicKey) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_VALIDATOR_PUBLIC_KEY => public_key,
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

    withdraw_validator_reward(public_key);
}
