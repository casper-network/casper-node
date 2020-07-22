#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casperlabs_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{URef, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_DESTINATION: &str = "destination";

#[no_mangle]
pub extern "C" fn call() {
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let destination_name: String = runtime::get_named_arg(ARG_DESTINATION);

    let source: URef = account::get_main_purse();
    let destination = system::create_purse();
    system::transfer_from_purse_to_purse(source, destination, amount).unwrap_or_revert();
    runtime::put_key(&destination_name, destination.into());
}
