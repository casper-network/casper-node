#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use casper_contract::contract_api::{runtime, storage};

const HASH_RESULT: &str = "hash_result";

const ARG_BYTES: &str = "bytes";

#[no_mangle]
pub extern "C" fn call() {
    let bytes: Vec<u8> = runtime::get_named_arg(ARG_BYTES);
    let hash = runtime::blake2b(bytes);
    let uref = storage::new_uref(hash);
    runtime::put_key(HASH_RESULT, uref.into())
}
