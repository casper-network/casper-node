#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::auction, PublicKey, U512};


fn cancel_reservations(validator: PublicKey, delegators: &[PublicKey]) {
    todo!();
}

// Remove delegators from validator's reserved list.
//
// Accepts delegators' and validator's public keys.
// Issues a cancel_reservations request to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let delegators: Vec<PublicKey> = runtime::get_named_arg(auction::ARG_DELEGATORS);
    let validator = runtime::get_named_arg(auction::ARG_VALIDATOR);

    cancel_reservations(validator, &delegators);
}
