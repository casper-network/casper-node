#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, storage};

const NEXT_ADDRESS_RESULT: &str = "next_address_result";

#[no_mangle]
pub extern "C" fn call() {
    let next_address = runtime::next_address();
    let uref = storage::new_uref(next_address);
    runtime::put_key(NEXT_ADDRESS_RESULT, uref.into())
}
