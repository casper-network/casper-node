#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::contract_api::{runtime, storage};

const ARG_UREF_NAME: &str = "uref_name";
const INITIAL_DATA: &str = "bawitdaba";

#[no_mangle]
pub extern "C" fn call() {
    let uref_name: String = runtime::get_named_arg(ARG_UREF_NAME);
    let uref = storage::new_uref(String::from(INITIAL_DATA));
    runtime::put_key(&uref_name, uref.into());
}
