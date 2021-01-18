#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;
use casper_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::NamedKeys,
    mint::{ACCESS_KEY, HASH_KEY},
    CLValue, ContractHash, ContractPackageHash, ContractVersion, URef,
};

pub const MODIFIED_MINT_EXT_FUNCTION_NAME: &str = "modified_mint_ext";
pub const POS_EXT_FUNCTION_NAME: &str = "pos_ext";

#[no_mangle]
pub extern "C" fn mint() {
    modified_mint::mint();
}

#[no_mangle]
pub extern "C" fn create() {
    modified_mint::create();
}

#[no_mangle]
pub extern "C" fn balance() {
    modified_mint::balance();
}

#[no_mangle]
pub extern "C" fn transfer() {
    modified_mint::transfer();
}

#[no_mangle]
pub extern "C" fn read_base_round_reward() {
    modified_mint::read_base_round_reward();
}

fn upgrade_mint() -> (ContractHash, ContractVersion) {
    let mint_package_hash: ContractPackageHash = runtime::get_key(HASH_KEY)
        .expect("should have mint")
        .into_hash()
        .expect("should be hash")
        .into();
    let _mint_access_key: URef = runtime::get_key(ACCESS_KEY)
        .unwrap_or_revert()
        .into_uref()
        .expect("should be uref");

    let entry_points = modified_mint::get_entry_points();
    let named_keys = NamedKeys::new();
    storage::add_contract_version(mint_package_hash, entry_points, named_keys)
}

#[no_mangle]
pub extern "C" fn upgrade() {
    let mut upgrades: BTreeMap<ContractHash, ContractHash> = BTreeMap::new();

    {
        let old_mint_hash = system::get_mint();
        let (new_mint_hash, _contract_version) = upgrade_mint();
        upgrades.insert(old_mint_hash, new_mint_hash);
    }

    runtime::ret(CLValue::from_t(upgrades).unwrap());
}
