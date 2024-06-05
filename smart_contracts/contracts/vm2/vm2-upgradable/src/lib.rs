#![cfg_attr(target_arch = "wasm32", no_main)]

use casper_macros::casper;
use casper_sdk::host::Entity;
use casper_sdk::prelude::*;
use casper_sdk::{host, log};

const CURRENT_VERSION: &str = "v1";

/// This contract implements a simple flipper.
#[casper(contract_state)]
pub struct UpgradableContract {
    /// The current state of the flipper.
    value: u8,
    /// The owner of the contract.
    owner: Entity,
}

impl Default for UpgradableContract {
    fn default() -> Self {
        panic!("Unable to instantiate contract without a constructor");
    }
}

#[casper]
impl UpgradableContract {
    #[casper(constructor)]
    pub fn new(initial_value: u8) -> Self {
        let caller = host::get_caller();
        Self {
            value: initial_value,
            owner: caller,
        }
    }

    #[casper(constructor)]
    pub fn default() -> Self {
        Self::new(Default::default())
    }

    pub fn increment(&mut self) {
        self.value += 1;
    }

    pub fn get(&self) -> u8 {
        self.value
    }

    pub fn version(&self) -> &str {
        CURRENT_VERSION
    }

    pub fn perform_upgrade(&self, new_code: Vec<u8>) {
        log!("V1: starting upgrade process current value={}", self.value);
        // let new_code = host::casper_copy_input();
        log!("New code length: {}", new_code.len());
        log!("New code first 10 bytes: {:?}", &new_code[..10]);

        host::casper_upgrade(Some(&new_code), Some("migrate"), None).unwrap();
    }
}
