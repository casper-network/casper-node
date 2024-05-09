#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    addressable_entity::{EntryPoint, EntryPoints, Parameters},
    CLType, EntryPointAccess, EntryPointPayment, EntryPointType, Key,
};

const ENTRY_FUNCTION_NAME: &str = "delegate";
const HASH_KEY_NAME: &str = "do_nothing_hash";
const PACKAGE_HASH_KEY_NAME: &str = "do_nothing_package_hash";
const ACCESS_KEY_NAME: &str = "do_nothing_access";
const CONTRACT_VERSION: &str = "contract_version";

#[no_mangle]
pub extern "C" fn delegate() {
    let _named_keys = runtime::list_named_keys();
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::new(
            ENTRY_FUNCTION_NAME,
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Called,
            EntryPointPayment::Caller,
        );
        entry_points.add_entry_point(entry_point);
        entry_points
    };

    let (contract_hash, contract_version) = storage::new_contract(
        entry_points,
        None,
        Some(PACKAGE_HASH_KEY_NAME.into()),
        Some(ACCESS_KEY_NAME.into()),
        None,
    );

    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(HASH_KEY_NAME, Key::contract_entity_key(contract_hash));
}
