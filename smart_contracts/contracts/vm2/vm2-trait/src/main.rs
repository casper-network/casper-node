#![cfg_attr(target_arch = "wasm32", no_main)]

use casper_macros::casper;
use casper_sdk::log;
use vm2_trait::perform_test;

#[casper(export)]
pub fn call() {
    log!("Hello");
    perform_test();
    log!("🎉 Success");
}
