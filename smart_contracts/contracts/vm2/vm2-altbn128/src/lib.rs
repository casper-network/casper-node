#![no_std]
#![no_main]

extern crate alloc;

use casper_contract_sdk::prelude::*;

#[cfg(not(any(
    feature = "wasm_add_test",
    feature = "wasm_mul_test",
    feature = "wasm_pairing_test"
)))]
mod host;
#[cfg(not(any(
    feature = "wasm_add_test",
    feature = "wasm_mul_test",
    feature = "wasm_pairing_test"
)))]
use host::perform_tests;
#[cfg(any(
    feature = "wasm_add_test",
    feature = "wasm_mul_test",
    feature = "wasm_pairing_test"
))]
mod wasm;
#[cfg(any(
    feature = "wasm_add_test",
    feature = "wasm_mul_test",
    feature = "wasm_pairing_test"
))]
use wasm::perform_tests;

#[casper(contract_state)]
pub struct AltBn128Contract {}

impl Default for AltBn128Contract {
    fn default() -> Self {
        panic!("nope");
    }
}

#[casper]
impl AltBn128Contract {
    #[casper(constructor)]
    pub fn new() -> Self {
        perform_tests();
        Self {}
    }

    #[cfg(not(any(
        feature = "wasm_add_test",
        feature = "wasm_mul_test",
        feature = "wasm_pairing_test"
    )))]
    pub fn pairing(&self, raw: Vec<u8>) -> Result<bool, u32> {
        host::alt_bn128_pairing_raw(&raw).map_err(|e| u32::from(e))
    }
}
