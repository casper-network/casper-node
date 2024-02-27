#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::mint, RuntimeArgs, URef, U512};

fn burn(uref: URef, amount: U512) {
    let contract_hash = system::get_mint();
    let args = runtime_args! {
        mint::ARG_PURSE => uref,
        mint::ARG_AMOUNT => amount,
    };
    runtime::call_contract::<()>(contract_hash, mint::METHOD_BURN, args);
}

// Accepts a Vector of purse URefs. Burn tokens from provided URefs.
#[no_mangle]
pub extern "C" fn call() {
    let purse: URef = runtime::get_named_arg(mint::ARG_PURSE);
    let amount: U512 = runtime::get_named_arg(mint::ARG_AMOUNT);
    burn(purse, amount);
}
