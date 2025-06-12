#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_contract_macros::casper;
use casper_contract_sdk::{
    casper::{self, Entity},
    log,
};

const CURRENT_VERSION: &str = "v2";

mod v1 {
    //! This module contains the original version of the UpgradableContract.
    //!
    //! It is used to keep the name of `UpgradableContract` for the original version
    //! while allowing the new version to be named `UpgradableContractV2`.
    //!
    //! This keeps the UID consistent with the UID already in the global state.

    use casper_contract_sdk::prelude::*;

    #[derive(Debug, PanicOnDefault)]
    #[casper(contract_state)]
    pub struct UpgradableContract {
        /// The current state of the flipper.
        pub(crate) value: u8,
        /// The owner of the contract.
        pub(crate) owner: Entity,
    }
}

/// This contract implements a simple flipper.
#[derive(Debug)]
#[casper(contract_state)]
pub struct UpgradableContractV2 {
    /// The current state of the flipper.
    value: u64,
    /// The owner of the contract.
    owner: Entity,
}

impl From<v1::UpgradableContract> for UpgradableContractV2 {
    fn from(old: v1::UpgradableContract) -> Self {
        Self {
            value: old.value as u64,
            owner: old.owner,
        }
    }
}

impl Default for UpgradableContractV2 {
    fn default() -> Self {
        panic!("Unable to instantiate contract without a constructor");
    }
}

#[casper]
impl UpgradableContractV2 {
    #[casper(constructor)]
    pub fn new(initial_value: u64) -> Self {
        let caller = casper::get_caller();
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
        self.increment_by(1);
    }

    pub fn increment_by(&mut self, value: u64) {
        let old_value = self.value;
        self.value = value.wrapping_add(value);
        log!(
            "Incrementing value by {value} from {} to {}",
            old_value,
            self.value
        );
    }

    pub fn get(&self) -> u64 {
        self.value
    }

    pub fn version(&self) -> &str {
        CURRENT_VERSION
    }

    #[casper(ignore_state)]
    pub fn migrate() {
        log!("Reading old state...");
        let old_state: v1::UpgradableContract = casper::read_state().unwrap();
        log!("Old state {old_state:?}");
        let new_state = UpgradableContractV2::from(old_state);
        log!("Success! New state: {new_state:?}");
        casper::write_state(&new_state).unwrap();
    }

    #[casper(ignore_state)]
    pub fn perform_upgrade() {
        let new_code = casper::copy_input();
        log!("V2: New code length: {}", new_code.len());
        log!("V2: New code first 10 bytes: {:?}", &new_code[..10]);

        let upgrade_result = casper::upgrade(&new_code, Some("migrate"), None);
        log!("{:?}", upgrade_result);
    }
}
