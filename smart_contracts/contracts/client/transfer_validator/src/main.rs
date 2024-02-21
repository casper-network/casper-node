#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::auction, PublicKey, U512};

const ARG_VALIDATOR: &str = "validator";
const ARG_NEW_VALIDATOR: &str = "new_validator";

fn transfer_validator(validator: PublicKey, new_validator: PublicKey) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_VALIDATOR => validator,
        auction::ARG_NEW_VALIDATOR => new_validator
    };
    runtime::call_contract::<()>(contract_hash, auction::METHOD_TRANSFER_VALIDATOR, args);
}

// Transfer validator.
//
// Accepts current validator's public key and new validator's public key
// where the existing `ValidatorBid` and all related delegators should be transferred.
#[no_mangle]
pub extern "C" fn call() {
    let validator = runtime::get_named_arg(ARG_VALIDATOR);
    let new_validator = runtime::get_named_arg(ARG_NEW_VALIDATOR);
    transfer_validator(validator, new_validator);
}
