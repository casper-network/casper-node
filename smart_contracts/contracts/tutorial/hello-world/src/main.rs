#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use casper_contract::contract_api::{runtime, storage};
use casper_types::{Key, URef};

const KEY: &str = "special_value";
const ARG_MESSAGE: &str = "message";

fn store(value: String) {
    // Store `value` under a new unforgeable reference.
    let value_ref: URef = storage::new_uref(value);

    // Wrap the unforgeable reference in a value of type `Key`.
    let value_key: Key = value_ref.into();

    // Store this key under the name "special_value" in context-local storage.
    runtime::put_key(KEY, value_key);
}

// All session code must have a `call` entrypoint.
#[no_mangle]
pub extern "C" fn call() {
    // Get the optional first argument supplied to the argument.
    let value: String = runtime::get_named_arg(ARG_MESSAGE);
    store(value);
}
