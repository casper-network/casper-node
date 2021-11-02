#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::string::{String, ToString};

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    contracts::{EntryPoint, EntryPoints},
    CLType, CLTyped, EntryPointAccess, EntryPointType, Key, Parameter,
};

const ENTRY_FUNCTION_NAME: &str = "process_set_key";
const HASH_KEY_NAME: &str = "set_named_key";
const PACKAGE_HASH_KEY_NAME: &str = "set_named_key_package_hash";
const ACCESS_KEY_NAME: &str = "set_named_key_access";
const CONTRACT_VERSION: &str = "set_named_key_contract_version";
const NAMED_KEY: &str = "expected_named_key";
const ARG_VALUE_TO_SET: &str = "value_to_set";

#[no_mangle]
pub extern "C" fn process_set_key() {
    let value_to_set: String = runtime::get_named_arg(ARG_VALUE_TO_SET);
    let value_uref = storage::new_uref(value_to_set);

    runtime::put_key(NAMED_KEY, Key::from(value_uref));
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::new(
            ENTRY_FUNCTION_NAME.to_string(),
            vec![Parameter::new(ARG_VALUE_TO_SET, String::cl_type())],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        entry_points
    };

    let (contract_hash, contract_version) = storage::new_contract(
        entry_points,
        None,
        Some(PACKAGE_HASH_KEY_NAME.to_string()),
        Some(ACCESS_KEY_NAME.to_string()),
    );

    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(HASH_KEY_NAME, contract_hash.into());
}
