#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::ToString;

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    contracts::{ContractHash, Parameters},
    CLType, ContractVersion, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints,
};

const ENTRY_FUNCTION_NAME: &str = "delegate";
const CONTRACT_NAME: &str = "local_state_stored";
const CONTRACT_PACKAGE_KEY: &str = "contract_package";
const CONTRACT_ACCESS_KEY: &str = "access_key";
const CONTRACT_VERSION: &str = "contract_version";

#[no_mangle]
pub extern "C" fn delegate() {
    local_state::delegate()
}

fn store() -> (ContractHash, ContractVersion) {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point = EntryPoint::new(
            ENTRY_FUNCTION_NAME,
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );

        entry_points.add_entry_point(entry_point);

        entry_points
    };
    storage::new_contract(
        entry_points,
        None,
        Some(CONTRACT_PACKAGE_KEY.to_string()),
        Some(CONTRACT_ACCESS_KEY.to_string()),
    )
}

#[no_mangle]
pub extern "C" fn call() {
    let (contract_hash, contract_version) = store();
    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(CONTRACT_NAME, contract_hash.into());
}
