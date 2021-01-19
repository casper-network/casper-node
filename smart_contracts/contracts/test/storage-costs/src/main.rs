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
const NEW_UREF_FUNCTION: &str = "new_uref_function";
const PUT_KEY_FUNCTION: &str = "put_key_function";
const REMOVE_KEY_FUNCTION: &str = "remove_key_function";
const NEW_KEY_NAME: &str = "new_key";
const CREATE_CONTRACT_PACKAGE_AT_HASH_FUNCTION: &str = "create_contract_package_at_hash_function";
const CREATE_CONTRACT_USER_GROUP_FUNCTION_FUNCTION: &str = "create_contract_user_group_function";
const PROVISION_UREFS_FUNCTION: &str = "provision_urefs_function";
const ACCESS_KEY_NAME: &str = "access_key";
const REMOVE_CONTRACT_USER_GROUP_FUNCTION: &str = "remove_contract_user_group_function";
const LABEL_NAME: &str = "Label";
const NEW_UREF_SUBCALL_FUNCTION: &str = "new_uref_subcall";

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
pub extern "C" fn new_uref_function() {
    let _new_uref = storage::new_uref(0u64);
}

#[no_mangle]
pub extern "C" fn put_key_function() {
    runtime::put_key(NEW_KEY_NAME, Key::Hash([0; 32]));
}

#[no_mangle]
pub extern "C" fn remove_key_function() {
    runtime::remove_key(WRITE_KEY_NAME);
}

#[no_mangle]
pub extern "C" fn create_contract_package_at_hash_function() {
    let (_contract_package_hash, _access_key) = storage::create_contract_package_at_hash();
}

#[no_mangle]
pub extern "C" fn create_contract_user_group_function() {
    let contract_package_hash = runtime::get_key(CONTRACT_KEY_NAME)
        .and_then(Key::into_hash)
        .expect("should have package hash")
        .into();
    let _result = storage::create_contract_user_group(
        contract_package_hash,
        LABEL_NAME,
        0,
        Default::default(),
    )
    .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn provision_urefs_function() {
    let contract_package_hash = runtime::get_key(CONTRACT_KEY_NAME)
        .and_then(Key::into_hash)
        .expect("should have package hash")
        .into();
    let _result = storage::provision_contract_user_group_uref(contract_package_hash, LABEL_NAME)
        .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn remove_contract_user_group_function() {
    let contract_package_hash = runtime::get_key(CONTRACT_KEY_NAME)
        .and_then(Key::into_hash)
        .expect("should have package hash")
        .into();
    storage::remove_contract_user_group(contract_package_hash, LABEL_NAME).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn new_uref_subcall() {
    let contract_package_hash = runtime::get_key(CONTRACT_KEY_NAME)
        .and_then(Key::into_hash)
        .expect("should have package hash")
        .into();
    runtime::call_versioned_contract(
        contract_package_hash,
        None,
        NEW_UREF_FUNCTION,
        Default::default(),
    )
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

        let entry_point = EntryPoint::new(
            NEW_UREF_FUNCTION,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            PUT_KEY_FUNCTION,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            REMOVE_KEY_FUNCTION,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            CREATE_CONTRACT_PACKAGE_AT_HASH_FUNCTION,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            CREATE_CONTRACT_USER_GROUP_FUNCTION_FUNCTION,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            PROVISION_UREFS_FUNCTION,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            REMOVE_CONTRACT_USER_GROUP_FUNCTION,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        let entry_point = EntryPoint::new(
            NEW_UREF_SUBCALL_FUNCTION,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);

        entry_points
    };

    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();
    runtime::put_key(&HASH_KEY_NAME, contract_package_hash.into());

    let named_keys = {
        let mut named_keys = NamedKeys::new();

        let uref_for_writing = storage::new_uref(());
        named_keys.insert(WRITE_KEY_NAME.to_string(), uref_for_writing.into());

        let uref_for_adding = storage::new_uref(U512::zero());
        named_keys.insert(ADD_KEY_NAME.to_string(), uref_for_adding.into());

        named_keys.insert(
            CONTRACT_KEY_NAME.to_string(),
            Key::Hash(contract_package_hash.value()),
        );
        named_keys.insert(ACCESS_KEY_NAME.to_string(), access_uref.into());

        named_keys
    };

    let (contract_hash, _version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);
    runtime::put_key(&CONTRACT_KEY_NAME, contract_hash.into());
    runtime::put_key(&ACCESS_KEY_NAME, access_uref.into());
}
