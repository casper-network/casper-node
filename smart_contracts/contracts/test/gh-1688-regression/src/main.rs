#![no_main]
#![no_std]

extern crate alloc;

use alloc::string::ToString;
use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    contracts::Parameters, CLType, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints,
};

const METHOD_PUT_KEY: &str = "put_key";
const NEW_KEY_NAME: &str = "Hello";
const NEW_KEY_VALUE: &str = "World";
const CONTRACT_PACKAGE_KEY: &str = "contract_package";
const CONTRACT_HASH_KEY: &str = "contract_hash";

#[no_mangle]
fn put_key() {
    let value = storage::new_uref(NEW_KEY_VALUE);
    runtime::put_key(NEW_KEY_NAME, value.into());
}

#[no_mangle]
fn call() {
    let mut entry_points = EntryPoints::new();
    entry_points.add_entry_point(EntryPoint::new(
        METHOD_PUT_KEY,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));

    let (contract_hash, _version) = storage::new_contract(
        entry_points,
        None,
        Some(CONTRACT_PACKAGE_KEY.to_string()),
        None,
    );
    runtime::put_key(CONTRACT_HASH_KEY, contract_hash.into());
}
