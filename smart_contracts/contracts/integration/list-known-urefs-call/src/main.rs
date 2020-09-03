#![no_std]
#![no_main]

use casper_contract::{contract_api::runtime, unwrap_or_revert::UnwrapOrRevert};
use casper_types::{contracts::DEFAULT_ENTRY_POINT_NAME, ApiError, RuntimeArgs};

const LIST_NAMED_KEYS_KEY: &str = "list_named_keys";

#[no_mangle]
pub extern "C" fn call() {
    let contract_hash = runtime::get_key(LIST_NAMED_KEYS_KEY)
        .unwrap_or_revert_with(ApiError::GetKey)
        .into_hash()
        .unwrap_or_revert();

    // Call `define` part of the contract.
    runtime::call_contract(
        contract_hash,
        DEFAULT_ENTRY_POINT_NAME,
        RuntimeArgs::default(),
    )
}
