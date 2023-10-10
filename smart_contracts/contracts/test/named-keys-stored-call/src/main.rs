#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::{self, contract_api::runtime, unwrap_or_revert::UnwrapOrRevert};
use casper_types::{ContractHash, Key, RuntimeArgs};

const CONTRACT_HASH_NAME: &str = "contract_stored";

#[no_mangle]
pub extern "C" fn call() {
    let contract_hash = runtime::get_key(CONTRACT_HASH_NAME)
        .and_then(Key::into_hash)
        .map(ContractHash::new)
        .unwrap_or_revert();

    let entry_point: String = runtime::get_named_arg("entry_point");

    runtime::call_contract::<()>(contract_hash, &entry_point, RuntimeArgs::default());
}
