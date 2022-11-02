#![no_std]
#![no_main]

#[cfg(not(target_arch = "wasm32"))]
compile_error!("target arch should be wasm32: compile with '--target wasm32-unknown-unknown'");

// We need to explicitly import the std alloc crate and `alloc::string::String` as we're in a
// `no_std` environment.
extern crate alloc;

use casper_contract::{contract_api::storage, unwrap_or_revert::UnwrapOrRevert};

#[no_mangle]
pub extern "C" fn call() {
    let seed_uref = storage::new_dictionary("nctl_dictionary").unwrap_or_revert();
    storage::dictionary_put(seed_uref, "foo", 1u64);
}
