#![no_std]
#![no_main]

use casper_contract::contract_api::runtime;
use casper_types::CLValue;

#[no_mangle]
pub extern "C" fn call() {
    // Create some data to return
    let return_data = b"casper_ret test data";

    // Call casper_ret with the data as CLValue
    let cl_value = CLValue::from_t(return_data).unwrap();
    runtime::ret(cl_value);
}
