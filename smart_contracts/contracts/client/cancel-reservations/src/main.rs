#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::auction, PublicKey, U512};

const ARG_VALIDATOR: &str = "validator";
const ARG_DELEGATORS: &str = "delegators";

fn cancel_reservations(validator: PublicKey, delegators: &[PublicKey]) {
    let contract_hash = system::get_auction();
    for delegator in delegators {
        let args = runtime_args! {
            auction::ARG_DELEGATOR => delegator,
            auction::ARG_VALIDATOR => validator.clone(),
        };
        runtime::call_contract::<U512>(contract_hash, auction::METHOD_CANCEL_RESERVATION, args);
    }
}

// Remove delegators from validator's reserved list.
//
// Accepts delegators' and validator's public keys.
// Issues a cancel_reservation request to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let delegators: Vec<PublicKey> = runtime::get_named_arg(ARG_DELEGATORS);
    let validator = runtime::get_named_arg(ARG_VALIDATOR);

    cancel_reservations(validator, &delegators);
}
