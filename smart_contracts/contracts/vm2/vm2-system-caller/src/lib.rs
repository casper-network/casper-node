#![cfg_attr(target_family = "wasm", no_main)]

pub mod exports {
    use casper_contract_sdk::{
        casper::casper_system,
        prelude::*,
        types::{PublicKey, SystemContractOption},
    };

    #[casper(export)]
    pub fn call() -> String {
        use borsh;
        let input =
            borsh::to_vec(&(PublicKey::Ed25519([99; 32]),)).expect("Serialization to succeed");
        let (_output, result) = casper_system(SystemContractOption::ActivateBid.into(), &input);

        match result {
            Ok(_) => "success".to_string(),
            Err(err) => err.to_string(),
        }
    }
}
