#![no_std]
#![no_main]

#[cfg(not(target_arch = "wasm32"))]
compile_error!("target arch should be wasm32: compile with '--target wasm32-unknown-unknown'");

// This code imports necessary aspects of external crates that we will use in our contract code.
extern crate alloc;
// Importing Rust types.
use alloc::{collections::btree_map::BTreeMap, vec::Vec};
// Importing aspects of the Casper platform.
use casper_contract::contract_api::{runtime, storage};
// Importing specific Casper types.
use casper_types::{
    addressable_entity::{EntityEntryPoint as EntryPoint, EntryPoints},
    contracts::NamedKeys,
    CLType, EntryPointAccess, EntryPointPayment, EntryPointType, Key,
};
/// Constants for the keys pointing to values stored in the account's named keys.
const CONTRACT_PACKAGE_NAME: &str = "package_name";
const CONTRACT_VERSION_KEY: &str = "version";
const CONTRACT_KEY: &str = "key_putter";
/// Creating constants for the various contract entry points.
const ENTRY_POINT_PUT_KEY: &str = "put_key";

const NEW_KEY_NAME: &str = "key_name";
const NEW_KEY_VALUE: &str = "key_putter_v3";

#[no_mangle]
fn put_key() {
    let value = storage::new_uref(NEW_KEY_VALUE);
    runtime::put_key(NEW_KEY_NAME, value.into());
}

/// Entry point that executes automatically when a caller installs the contract.
#[no_mangle]
pub extern "C" fn call() {
    let package_key = runtime::get_key(CONTRACT_PACKAGE_NAME).unwrap(); //We are assuming that v1 was installed and it set up this package
    let named_keys = NamedKeys::new();

    // Create the entry points for this contract.
    let mut entry_points = EntryPoints::new();

    entry_points.add_entry_point(EntryPoint::new(
        ENTRY_POINT_PUT_KEY,
        Vec::new(),
        CLType::I32,
        EntryPointAccess::Public,
        EntryPointType::Called,
        EntryPointPayment::Caller,
    ));
    let contract_package_hash = match package_key {
        Key::Hash(hash_addr) => hash_addr,
        _ => panic!("shouldn't happen"),
    };
    let (contract_hash, contract_version) = storage::add_contract_version(
        contract_package_hash.into(),
        entry_points,
        named_keys,
        BTreeMap::new(),
    );
    let version_uref = storage::new_uref(contract_version);
    runtime::put_key(CONTRACT_VERSION_KEY, version_uref.into());

    // Create a named key for the contract hash.
    runtime::put_key(CONTRACT_KEY, contract_hash.into());
}
