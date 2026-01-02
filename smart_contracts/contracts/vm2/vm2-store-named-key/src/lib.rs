#![cfg_attr(target_family = "wasm", no_main)]

pub mod exports {
    use casper_contract_sdk::prelude::*;
    use casper_executor_wasm_common::keyspace::Keyspace;

    #[casper(export)]
    pub fn call(name: String, value: Vec<u8>) {
        casper::print(format!("Storing: {}, {:?}", name, value).as_str());
        let key = Keyspace::NamedValue(&name);
        casper::write(key, &value).unwrap();
    }
}
