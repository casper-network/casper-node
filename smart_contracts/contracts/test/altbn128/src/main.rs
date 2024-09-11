#![no_std]
#![no_main]

extern crate alloc;

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

#[no_mangle]
pub extern "C" fn call() {
    perform_tests();
}
