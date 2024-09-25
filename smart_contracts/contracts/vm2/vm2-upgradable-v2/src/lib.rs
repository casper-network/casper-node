#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_macros::casper;
use casper_sdk::{
    host::{self, Entity},
    log,
    serializers::borsh::BorshDeserialize,
};

const CURRENT_VERSION: &str = "v2";

#[derive(BorshDeserialize, Debug)]
#[borsh(crate = "casper_sdk::serializers::borsh")]
pub struct UpgradableContractV1 {
    /// The current state of the flipper.
    value: u8,
    /// The owner of the contract.
    owner: Entity,
}

impl Default for UpgradableContractV1 {
    fn default() -> Self {
        panic!("Unable to instantiate contract without a constructor");
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

impl From<UpgradableContractV1> for UpgradableContractV2 {
    fn from(old: UpgradableContractV1) -> Self {
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
        let old_state: UpgradableContractV1 = host::read_state().unwrap();
        log!("Old state {old_state:?}");
        let new_state = UpgradableContractV2::from(old_state);
        log!("Success! New state: {new_state:?}");
        host::write_state(&new_state).unwrap();
    }

    #[casper(ignore_state)]
    pub fn perform_upgrade() {
        let new_code = host::casper_copy_input();
        log!("V2: New code length: {}", new_code.len());
        log!("V2: New code first 10 bytes: {:?}", &new_code[..10]);

        let upgrade_result = host::casper_upgrade(&new_code, Some("migrate"), None);
        log!("{:?}", upgrade_result);
    }
}
