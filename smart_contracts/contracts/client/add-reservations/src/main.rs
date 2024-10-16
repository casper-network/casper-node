#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use casper_contract::contract_api::{runtime, system};
use casper_types::{
    runtime_args,
    system::auction::{self, Reservation},
};

fn add_reservations(reservations: Vec<Reservation>) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_RESERVATIONS => reservations,
    };
    runtime::call_contract::<()>(contract_hash, auction::METHOD_ADD_RESERVATIONS, args);
}

// Add delegators to validator's reserved list.
//
// Accepts reservations.
// Issues an add_reservations request to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let reservations: Vec<Reservation> = runtime::get_named_arg(auction::ARG_RESERVATIONS);

    add_reservations(reservations);
}
