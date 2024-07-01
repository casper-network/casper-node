#![no_main]
#![no_std]

extern crate alloc;

use alloc::string::ToString;
use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    addressable_entity::Parameters, CLType, EntryPoint, EntryPointAccess, EntryPointPayment,
    EntryPointType, EntryPoints, Key,
};

const METHOD_TEST_ENTRY_POINT: &str = "test_entry_point";
const NEW_KEY_NAME: &str = "Hello";
const NEW_KEY_VALUE: &str = "World";
const CONTRACT_PACKAGE_KEY: &str = "contract_package";
const CONTRACT_HASH_KEY: &str = "contract_hash";

#[no_mangle]
fn test_entry_point() {
    let value = storage::new_uref(NEW_KEY_VALUE);
    runtime::put_key(NEW_KEY_NAME, value.into());
}

#[no_mangle]
fn call() {
    let mut entry_points = EntryPoints::new();
    entry_points.add_entry_point(EntryPoint::new(
        METHOD_TEST_ENTRY_POINT,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Called,
        EntryPointPayment::Caller,
    ));

    let (contract_hash, _version) = storage::new_contract(
        entry_points,
        None,
        Some(CONTRACT_PACKAGE_KEY.to_string()),
        None,
        None,
    );
    runtime::put_key(CONTRACT_HASH_KEY, Key::contract_entity_key(contract_hash));
}
