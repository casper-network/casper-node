#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::{ContractHash, ContractPackageHash, NamedKeys, Parameters},
    CLType, ContractVersion, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints,
};

const CONTRACT_NAME: &str = "local_state_stored";
const ENTRY_FUNCTION_NAME: &str = "delegate";
const CONTRACT_PACKAGE_KEY: &str = "contract_package";
const CONTRACT_ACCESS_KEY: &str = "access_key";
const CONTRACT_VERSION: &str = "contract_version";

#[no_mangle]
pub extern "C" fn delegate() {
    local_state_stored_upgraded::delegate()
}

fn upgrade(contract_package_hash: ContractPackageHash) -> (ContractHash, ContractVersion) {
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

    storage::add_contract_version(contract_package_hash, entry_points, NamedKeys::new())
}

#[no_mangle]
pub extern "C" fn call() {
    let contract_package_hash = runtime::get_named_arg(CONTRACT_PACKAGE_KEY);
    let _access_key = runtime::get_key(CONTRACT_ACCESS_KEY)
        .unwrap_or_revert()
        .into_uref()
        .unwrap_or_revert();
    let (contract_hash, contract_version) = upgrade(contract_package_hash);
    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(CONTRACT_NAME, contract_hash.into());
}
