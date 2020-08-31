#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::contract_api::runtime;
use casper_types::U512;

const ARG_VALUE0: &str = "value0";
const ARG_VALUE1: &str = "value1";

#[no_mangle]
pub extern "C" fn call() {
    let value0: String = runtime::get_named_arg(ARG_VALUE0);
    assert_eq!(value0, "Hello, world!");

    let value1: U512 = runtime::get_named_arg(ARG_VALUE1);
    assert_eq!(value1, U512::from(42));
}
