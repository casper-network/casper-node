#![no_std]
#![no_main]

#[cfg(not(target_arch = "wasm32"))]
compile_error!("target arch should be wasm32: compile with '--target wasm32-unknown-unknown'");

extern crate alloc;

use casper_types::{AddressableEntityHash, ApiError, Key, RuntimeArgs};

use casper_contract::{contract_api::runtime, unwrap_or_revert::UnwrapOrRevert};

const COUNTER_KEY: &str = "counter";
const COUNTER_INC: &str = "counter_inc";
const COUNTER_GET: &str = "counter_get";

#[no_mangle]
pub extern "C" fn call() {
    // Read the Counter smart contract's ContractHash.
    let contract_hash = {
        let counter_uref = runtime::get_key(COUNTER_KEY).unwrap_or_revert_with(ApiError::GetKey);
        if let Key::AddressableEntity(hash) = counter_uref {
            AddressableEntityHash::new(hash.value())
        } else {
            runtime::revert(ApiError::User(66));
        }
    };

    // Call Counter to get the current value.
    let current_counter_value: u32 =
        runtime::call_contract(contract_hash, COUNTER_GET, RuntimeArgs::new());

    // Call Counter to increment the value.
    let _: () = runtime::call_contract(contract_hash, COUNTER_INC, RuntimeArgs::new());

    // Call Counter to get the new value.
    let new_counter_value: u32 =
        runtime::call_contract(contract_hash, COUNTER_GET, RuntimeArgs::new());

    // Expect counter to increment by one.
    if new_counter_value - current_counter_value != 1u32 {
        runtime::revert(ApiError::User(67));
    }
}
