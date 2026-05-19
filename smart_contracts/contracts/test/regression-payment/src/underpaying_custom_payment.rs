#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;

use casper_contract::contract_api::{runtime, storage};
use casper_types::{bytesrepr::Bytes, Phase};

const ARG_ITERATIONS: &str = "iterations";
const SCRATCH_BYTES: usize = 4096;

#[no_mangle]
pub extern "C" fn call() {
    if runtime::get_phase() != Phase::Payment {
        return;
    }

    let iterations: u32 = runtime::get_named_arg(ARG_ITERATIONS);
    let scratch: Bytes = vec![0u8; SCRATCH_BYTES].into();
    let scratch_uref = storage::new_uref(scratch.clone());
    for _ in 0..iterations {
        storage::write(scratch_uref, scratch.clone());
    }
}
