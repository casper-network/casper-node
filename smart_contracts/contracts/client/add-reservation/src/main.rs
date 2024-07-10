#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::auction, PublicKey, U512};

const ARG_VALIDATOR: &str = "validator";
const ARG_DELEGATOR: &str = "delegator";

fn add_reservation(delegator: PublicKey, validator: PublicKey) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_DELEGATOR => delegator,
        auction::ARG_VALIDATOR => validator,
    };
    runtime::call_contract::<U512>(contract_hash, auction::METHOD_ADD_RESERVATION, args);
}

// Add delegator to validator's reserved list.
//
// Accepts delegator's and validator's public keys.
// Issues an add_reservation request to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let delegator = runtime::get_named_arg(ARG_DELEGATOR);
    let validator = runtime::get_named_arg(ARG_VALIDATOR);

    add_reservation(delegator, validator);
}
