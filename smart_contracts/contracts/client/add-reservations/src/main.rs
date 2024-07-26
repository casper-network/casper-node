#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use casper_contract::contract_api::runtime;
use casper_types::{system::auction::{self, DelegationRate}, PublicKey};

fn add_reservations(_validator: PublicKey, _delegators: Vec<(PublicKey, DelegationRate)>) {
    todo!();
}

// Add delegators to validator's reserved list.
//
// Accepts delegators' and validator's public keys.
// Issues an add_reservations request to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let delegators: Vec<PublicKey> = runtime::get_named_arg(auction::ARG_DELEGATORS);
    let validator = runtime::get_named_arg(auction::ARG_VALIDATOR);

    add_reservations(validator, delegators);
}
