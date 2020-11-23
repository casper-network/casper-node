#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, storage};

const INPUT_LENGTH: usize = 32;

const HASH_RESULT: &str = "hash_result";

const ARG_BYTES: &str = "bytes";

#[no_mangle]
pub extern "C" fn call() {
    let bytes: [u8; INPUT_LENGTH] = runtime::get_named_arg(ARG_BYTES);
    let hash = runtime::blake2b(bytes);
    let uref = storage::new_uref(hash);
    runtime::put_key(HASH_RESULT, uref.into())
}
