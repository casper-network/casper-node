#![no_std]
#![no_main]

extern crate alloc;
use alloc::string::String;

use casper_contract::contract_api::runtime;
use casper_types::Key;

const ARG_DATA: &str = "data";

#[no_mangle]
fn is_key(key_str: &str) -> bool {
    Key::from_formatted_str(key_str).is_ok()
}

#[no_mangle]
pub extern "C" fn call() {
    let data: String = runtime::get_named_arg(ARG_DATA);

    assert!(is_key(&data), "Data should be a key");
}
