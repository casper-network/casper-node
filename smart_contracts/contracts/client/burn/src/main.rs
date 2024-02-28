#![no_std]
#![no_main]

extern crate alloc;
use alloc::string::String;

use casper_contract::{
    contract_api::{account},
};

use casper_contract::{
    contract_api::{runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{runtime_args, system::mint, RuntimeArgs, URef, U512};

const ARG_BURN_AMOUNT : &str = "burn_amount";
const ARG_PURSE_NAME: &str = "purse_name";

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
    let purse_name: String = runtime::get_named_arg(ARG_PURSE_NAME);
    let amount: U512 = runtime::get_named_arg(mint::ARG_AMOUNT);
    let burn_amount: U512 = runtime::get_named_arg(ARG_BURN_AMOUNT);

    let purse_uref = runtime::get_key(&purse_name).unwrap();

    burn(purse_uref, amount);
}
