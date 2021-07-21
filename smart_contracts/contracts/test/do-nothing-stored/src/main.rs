#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::string::{String, ToString};

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    contracts::{EntryPoint, EntryPoints},
    CLType, CLTyped, EntryPointAccess, EntryPointType, Parameter,
};

const ENTRY_FUNCTION_NAME: &str = "delegate";
const HASH_KEY_NAME: &str = "do_nothing_hash";
const PACKAGE_HASH_KEY_NAME: &str = "do_nothing_package_hash";
const ACCESS_KEY_NAME: &str = "do_nothing_access";
const CONTRACT_VERSION: &str = "contract_version";
const ARG_PURSE_NAME: &str = "purse_name";

#[no_mangle]
pub extern "C" fn delegate() {}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::new(
            ENTRY_FUNCTION_NAME.to_string(),
            vec![Parameter::new(ARG_PURSE_NAME, String::cl_type())],
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
