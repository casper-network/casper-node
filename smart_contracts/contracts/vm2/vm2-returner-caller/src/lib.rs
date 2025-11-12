#![cfg_attr(target_family = "wasm", no_main)]

pub mod exports {
    use casper_contract_sdk::{prelude::*, types::Address};

    #[casper(export)]
    pub fn call(address: Address) {
        let _ = casper::print(&format!("trying to call address {:?}", address));

        casper::casper_call(&address, 0, "do_return", &[])
            .1
            .unwrap();
        casper::casper_call(&address, 0, "do_not_return", &[])
            .1
            .unwrap();
    }
}
