#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use casper_contract::contract_api::runtime;
use casper_types::system::auction::{Reservation, ARG_RESERVATIONS};

fn add_reservations(_reservations: Vec<Reservation>) {
    todo!();
}

// Add delegators to validator's reserved list.
//
// Accepts reservations.
// Issues an add_reservations request to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let reservations: Vec<Reservation> = runtime::get_named_arg(ARG_RESERVATIONS);

    add_reservations(reservations);
}
