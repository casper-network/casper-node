#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::ToString, vec::Vec};

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::NamedKeys, CLType, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
    U512,
};

const WRITE_FUNCTION_SMALL_NAME: &str = "write_function_small";
const WRITE_FUNCTION_LARGE_NAME: &str = "write_function_large";
const ADD_FUNCTION_SMALL_NAME: &str = "add_function_small";
const ADD_FUNCTION_LARGE_NAME: &str = "add_function_large";
const WRITE_KEY_NAME: &str = "write";
const ADD_KEY_NAME: &str = "add";
const WRITE_SMALL_VALUE: &[u8] = b"1";
const WRITE_LARGE_VALUE: &[u8] = b"1111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111";
const HASH_KEY_NAME: &str = "contract_package";
const CONTRACT_KEY_NAME: &str = "contract";
const ADD_SMALL_VALUE: u64 = 1;
const ADD_LARGE_VALUE: u64 = u64::max_value();

// Executes the named key functions from the `runtime` module and most of the functions from the
// `storage` module.
#[no_mangle]
pub extern "C" fn write_function_small() {
    let uref = runtime::get_key(WRITE_KEY_NAME)
        .and_then(Key::into_uref)
        .unwrap_or_revert();
    storage::write(uref, WRITE_SMALL_VALUE.to_vec());
}

#[no_mangle]
pub extern "C" fn write_function_large() {
    let uref = runtime::get_key(WRITE_KEY_NAME)
        .and_then(Key::into_uref)
        .unwrap_or_revert();
    storage::write(uref, WRITE_LARGE_VALUE.to_vec());
}

#[no_mangle]
pub extern "C" fn add_function_small() {
    let uref = runtime::get_key(ADD_KEY_NAME)
        .and_then(Key::into_uref)
        .unwrap_or_revert();
    storage::add(uref, U512::from(ADD_SMALL_VALUE));
}

#[no_mangle]
pub extern "C" fn add_function_large() {
    let uref = runtime::get_key(ADD_KEY_NAME)
        .and_then(Key::into_uref)
        .unwrap_or_revert();
    storage::add(uref, U512::from(ADD_LARGE_VALUE));
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::new(
            WRITE_FUNCTION_SMALL_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point = EntryPoint::new(
            WRITE_FUNCTION_LARGE_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point = EntryPoint::new(
            ADD_FUNCTION_SMALL_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point = EntryPoint::new(
            ADD_FUNCTION_LARGE_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        entry_points
    };

    let (contract_package_hash, _access_uref) = storage::create_contract_package_at_hash();
    runtime::put_key(&HASH_KEY_NAME, contract_package_hash.into());

    let named_keys = {
        let mut named_keys = NamedKeys::new();

        let uref_for_writing = storage::new_uref(());
        named_keys.insert(WRITE_KEY_NAME.to_string(), uref_for_writing.into());

        let uref_for_adding = storage::new_uref(U512::zero());
        named_keys.insert(ADD_KEY_NAME.to_string(), uref_for_adding.into());

        named_keys
    };

    let (contract_hash, _version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);
    runtime::put_key(&CONTRACT_KEY_NAME, contract_hash.into());
}
