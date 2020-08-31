#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{Key, URef, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_NAME: &str = "name";

#[no_mangle]
pub extern "C" fn call() {
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let name: String = runtime::get_named_arg(ARG_NAME);
    let main_purse: URef = account::get_main_purse();
    let new_purse: URef = system::create_purse();

    system::transfer_from_purse_to_purse(main_purse, new_purse, amount).unwrap_or_revert();

    let new_purse_key: Key = new_purse.into();
    runtime::put_key(&name, new_purse_key);
}
