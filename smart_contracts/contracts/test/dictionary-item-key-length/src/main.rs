#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};

const OVERSIZED_DICTIONARY_ITEM_KEY: &str = "nZ1a27wa2MYty0KpPcl9WOYAFygPUWSqSTN5hyDi1MlfOk2RmykDdwM4HENeXEIUlnZ1a27wa2MYty0KpPcl9WOYAFygPUWSqSTN5hyDi1MlfOk2RmykDdwM4HENeXEIUl";
const DICTIONARY_NAME: &str = "dictionary-name";
const DICTIONARY_VALUE: &str = "dictionary-value";
const DICTIONARY_OP: &str = "dictionary-operation";
const OP_PUT: &str = "put";
const OP_GET: &str = "get";

#[no_mangle]
pub extern "C" fn call() {
    let dictionary_uref = storage::new_dictionary(DICTIONARY_NAME).unwrap_or_revert();
    let operation: String = runtime::get_named_arg(DICTIONARY_OP);
    if operation == OP_GET {
        let _ = storage::dictionary_get::<String>(dictionary_uref, OVERSIZED_DICTIONARY_ITEM_KEY);
    } else if operation == OP_PUT {
        storage::dictionary_put(
            dictionary_uref,
            OVERSIZED_DICTIONARY_ITEM_KEY,
            DICTIONARY_VALUE,
        );
    }
}
