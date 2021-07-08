#![no_std]
#![no_main]
// Required to bring `#[panic_handler]` from `contract::handlers` into scope.
#![allow(unused_imports, clippy::single_component_path_imports)]
use casper_contract;

#[no_mangle]
pub extern "C" fn call() {
    // Does nothing
}
