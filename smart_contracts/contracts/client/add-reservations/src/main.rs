#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::auction, PublicKey};

fn add_reservations(validator: &PublicKey, delegators: &[PublicKey]) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_VALIDATOR => validator,
        auction::ARG_DELEGATORS => delegators,
    };
    runtime::call_contract::<()>(contract_hash, auction::METHOD_ADD_RESERVATIONS, args);
}

// Add delegators to validator's reserved list.
//
// Accepts delegators' and validator's public keys.
// Issues an add_reservations request to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let delegators: Vec<PublicKey> = runtime::get_named_arg(auction::ARG_DELEGATORS);
    let validator = runtime::get_named_arg(auction::ARG_VALIDATOR);

    add_reservations(&validator, &delegators);
}
