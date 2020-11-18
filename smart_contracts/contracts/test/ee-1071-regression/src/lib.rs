#![no_std]

use casper_contract::contract_api::{runtime, storage};

#[no_mangle]
pub extern "C" fn new_uref() {
    let new_uref = storage::new_uref(0);
    runtime::put_key(&new_uref.to_formatted_string(), new_uref.into())
}
