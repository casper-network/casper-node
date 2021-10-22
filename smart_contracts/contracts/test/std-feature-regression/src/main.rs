#![no_std]
#![no_main]

use casper_contract::contract_api::runtime;

#[no_mangle]
pub extern "C" fn call() {
    let _named_keys = runtime::list_named_keys();
}
