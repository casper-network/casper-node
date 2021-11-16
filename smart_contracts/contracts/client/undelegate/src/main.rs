#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::auction, PublicKey, RuntimeArgs, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_DELEGATOR: &str = "delegator";
const ARG_VALIDATOR: &str = "validator";
const ARG_NEW_VALIDATOR_PUBLIC_KEY: &str = "new_validator_public_key";

fn undelegate(
    delegator: PublicKey,
    validator: PublicKey,
    amount: U512,
    new_validator_public_key: Option<PublicKey>,
) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_DELEGATOR => delegator,
        auction::ARG_VALIDATOR => validator,
        auction::ARG_AMOUNT => amount,
        auction::ARG_NEW_VALIDATOR_PUBLIC_KEY => new_validator_public_key
    };
    let _amount: U512 = runtime::call_contract(contract_hash, auction::METHOD_UNDELEGATE, args);
}

// Undelegate contract.
//
// Accepts a delegator's public key, validator's public key to be undelegated, and an amount
// to withdraw (of type `U512`).
#[no_mangle]
pub extern "C" fn call() {
    let delegator = runtime::get_named_arg(ARG_DELEGATOR);
    let validator = runtime::get_named_arg(ARG_VALIDATOR);
    let amount = runtime::get_named_arg(ARG_AMOUNT);
    let new_validator_public_key = runtime::get_named_arg(ARG_NEW_VALIDATOR_PUBLIC_KEY);
    undelegate(delegator, validator, amount, new_validator_public_key);
}
