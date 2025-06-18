#![no_std]
#![no_main]

#[cfg(not(target_arch = "wasm32"))]
compile_error!("target arch should be wasm32: compile with '--target wasm32-unknown-unknown'");

// This code imports necessary aspects of external crates that we will use in our contract code.
extern crate alloc;
// Importing Rust types.
use alloc::{
    collections::btree_map::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};
// Importing aspects of the Casper platform.
use casper_contract::contract_api::{runtime, storage};
// Importing specific Casper types.
use casper_types::{
    addressable_entity::{EntityEntryPoint as EntryPoint, EntryPoints},
    bytesrepr::FromBytes,
    contracts::NamedKeys,
    ApiError, CLType, CLTyped, EntryPointAccess, EntryPointPayment, EntryPointType, Key, URef,
};
/// Constants for the keys pointing to values stored in the account's named keys.
const CONTRACT_PACKAGE_NAME: &str = "package_name";
const CONTRACT_ACCESS_UREF: &str = "access_uref";

/// Creating constants for the various contract entry points.
const ENTRY_POINT_PUT_KEY: &str = "put_key";

/// Constants for the keys pointing to values stored in the contract's named keys.
const CONTRACT_VERSION_KEY: &str = "version";
const ALL_CONTRACTS_COUNTER: &str = "all_contracts_counter";
const CONTRACT_KEY: &str = "key_putter";

const KEY_PLACEHOLDER: &str = "key_placeholder";

#[no_mangle]
fn put_key() {
    let named_keys = runtime::list_named_keys();
    let mut number_of_matches = 0;
    for key in named_keys.names() {
        if key.to_string().starts_with("v_") {
            number_of_matches += 1;
        }
    }
    let key = if number_of_matches <= 0 {
        "Contract not installed?".to_string()
    } else {
        format!("v_{number_of_matches}")
    };
    let value_to_store = match get_stored_value::<String>(&key) {
        Some(value_to_store) => value_to_store,
        None => format!("Nothing found under key {key}"),
    };
    let value = storage::new_uref(value_to_store);
    runtime::put_key(KEY_PLACEHOLDER, value.into());
}

pub fn install(contract_version: u32) {
    let mut named_keys = NamedKeys::new();
    let key = format!("v_{contract_version}");
    let value = format!("key_putter_v{contract_version}");
    named_keys.insert(key, storage::new_uref(value).into());
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

    // Create a new contract package that can be upgraded.
    let (stored_contract_hash, contract_version) = storage::new_contract(
        entry_points,
        Some(named_keys),
        Some(CONTRACT_PACKAGE_NAME.to_string()),
        Some(CONTRACT_ACCESS_UREF.to_string()),
        None,
    );

    // Store the contract version in the context's named keys.
    let version_uref = storage::new_uref(contract_version);
    runtime::put_key(CONTRACT_VERSION_KEY, version_uref.into());

    // Create a named key for the contract hash.
    runtime::put_key(CONTRACT_KEY, stored_contract_hash.into());

    let all_contracts_counter_uref = storage::new_uref(contract_version);
    runtime::put_key(ALL_CONTRACTS_COUNTER, all_contracts_counter_uref.into());
}

pub fn upgrade(contract_version: u32) {
    let package_key = runtime::get_key(CONTRACT_PACKAGE_NAME).unwrap();
    let mut named_keys = NamedKeys::new();
    let key = format!("v_{contract_version}");
    let value = format!("key_putter_v{contract_version}");
    named_keys.insert(key, storage::new_uref(value).into());
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
    let (contract_hash, updated_contract_version) = storage::add_contract_version(
        contract_package_hash.into(),
        entry_points,
        named_keys,
        BTreeMap::new(),
    );
    let version_uref = storage::new_uref(updated_contract_version);
    runtime::put_key(CONTRACT_VERSION_KEY, version_uref.into());

    // Create a named key for the contract hash.
    runtime::put_key(CONTRACT_KEY, contract_hash.into());

    let all_contracts_counter_uref = storage::new_uref(contract_version);
    runtime::put_key(ALL_CONTRACTS_COUNTER, all_contracts_counter_uref.into());
}

/// Entry point that executes automatically when a caller installs the contract.
#[no_mangle]
pub extern "C" fn call() {
    let package_key = runtime::get_key(CONTRACT_PACKAGE_NAME);
    if package_key.is_none() {
        //install
        install(1);
    } else {
        let all_contracts_counter = get_stored_value::<u32>(ALL_CONTRACTS_COUNTER).unwrap();
        upgrade(all_contracts_counter + 1)
    }
}

/// Reads value from a named key.
pub fn get_stored_value<T>(name: &str) -> Option<T>
where
    T: FromBytes + CLTyped,
{
    let uref = get_uref(name);
    storage::read(uref).unwrap()
}

/// Gets [`URef`] under a name.
fn get_uref(name: &str) -> URef {
    let key = runtime::get_key(name).ok_or(ApiError::MissingKey).unwrap();
    key.try_into().unwrap()
}
