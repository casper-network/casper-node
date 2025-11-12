#![cfg_attr(target_family = "wasm", no_main)]

pub mod exports {
    use casper_contract_sdk::prelude::*;

    #[casper(export)]
    pub fn call() {
        let _ = casper::print("hello!");
    }
}
