#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

pub mod exports {
    use casper_contract_sdk::{casper_executor_wasm_common::keyspace::Keyspace, prelude::*};

    #[casper(export)]
    pub fn call(weight: u8) {
        let account_hash_bytes = [10u8; 32];
        if weight == 0 {
            let keyspace = Keyspace::RemoveAssociatedKeys(&account_hash_bytes);
            casper::remove(keyspace).unwrap()
        } else {
            let keyspace = Keyspace::AssociatedKeys(&account_hash_bytes);
            casper::write(keyspace, &[weight]).unwrap();
        }
    }
}
