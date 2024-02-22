#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{
    runtime_args,
    system::auction::{ARG_NEW_VALIDATOR, ARG_VALIDATOR, METHOD_TRANSFER_VALIDATOR},
    PublicKey,
};

fn transfer_validator(validator: PublicKey, new_validator: PublicKey) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        ARG_VALIDATOR => validator,
        ARG_NEW_VALIDATOR => new_validator
    };
    runtime::call_contract::<()>(contract_hash, METHOD_TRANSFER_VALIDATOR, args);
}

// Transfer validator.
//
// Accepts current validator's public key and new validator's public key.
// Updates existing validator bid and all related delegator bids with
// the new validator's public key.
#[no_mangle]
pub extern "C" fn call() {
    let validator = runtime::get_named_arg(ARG_VALIDATOR);
    let new_validator = runtime::get_named_arg(ARG_NEW_VALIDATOR);
    transfer_validator(validator, new_validator);
}
