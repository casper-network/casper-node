#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::mint, RuntimeArgs, URef};

fn burn(urefs: Vec<URef>) {
    let contract_hash = system::get_mint();
    let args = runtime_args! {
        mint::ARG_PURSES => urefs,
    };
    runtime::call_contract::<()>(contract_hash, mint::METHOD_BURN, args);
}

// Accepts a public key. Issues an activate-bid bid to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let urefs: Vec<URef> = runtime::get_named_arg(mint::ARG_PURSES);
    burn(urefs);
}
