#![no_main]
#![no_std]

extern crate alloc;

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{runtime_args, RuntimeArgs, U512};

pub const BURN_ENTRYPOINT: &str = "burn";
pub const ARG_PURSE: &str = "purse";
pub const ARG_AMOUNT : &str = "amount";

#[no_mangle]
pub extern "C" fn call() {
    let caller_purse = account::get_main_purse();
    let new_purse = system::create_purse();
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    system::transfer_from_purse_to_purse(
        caller_purse,
        new_purse,
        amount,
        None
    ).unwrap_or_revert();

    let _: () = runtime::call_contract(
        system::get_mint(),
        BURN_ENTRYPOINT,
        runtime_args! {
            ARG_PURSE => system_purse,
            ARG_AMOUNT => amount,
        },
    );
}