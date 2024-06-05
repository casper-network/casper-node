#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_macros::casper;
use casper_sdk::{host, log};

/// This contract implements a simple flipper.
#[casper(contract_state)]
pub struct UpgradableContract {
    /// The current state of the flipper.
    value: u8,
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
        Self {
            value: initial_value,
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

    #[casper(ignore_state)]
    pub fn upgrade() {
        let new_code = host::casper_copy_input();
        log!("New code length: {}", new_code.len());
        log!("New code first 10 bytes: {:?}", &new_code[..10]);
        todo!()
        // host::upgrade_contract(Some(&new_code));
        // host::upgrade_contract("vm2-flipper copy")
    }
}
