#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::ToString, vec::Vec};

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    addressable_entity::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, NamedKeys},
    package::ENTITY_INITIAL_VERSION,
    runtime_args, AddressableEntityHash, CLType, EntityVersion, Key, PackageHash,
};

const PACKAGE_HASH_KEY: &str = "package_hash_key";
const PACKAGE_ACCESS_KEY: &str = "package_access_key";
const CONTRACT_HASH_KEY: &str = "contract_hash_key";
const CONTRACT_CODE: &str = "contract_code_test";
const SESSION_CODE: &str = "session_code_test";
const NEW_KEY: &str = "new_key";
const NAMED_KEY: &str = "contract_named_key";
const CONTRACT_VERSION: &str = "contract_version";

#[no_mangle]
pub extern "C" fn session_code_test() {
    assert!(runtime::get_key(PACKAGE_HASH_KEY).is_some());
    assert!(runtime::get_key(PACKAGE_ACCESS_KEY).is_some());
    assert!(runtime::get_key(NAMED_KEY).is_none());
}

#[no_mangle]
pub extern "C" fn contract_code_test() {
    assert!(runtime::get_key(PACKAGE_HASH_KEY).is_none());
    assert!(runtime::get_key(PACKAGE_ACCESS_KEY).is_none());
    assert!(runtime::get_key(NAMED_KEY).is_some());
}

#[no_mangle]
pub extern "C" fn session_code_caller_as_session() {
    let contract_package_hash = runtime::get_key(PACKAGE_HASH_KEY)
        .expect("should have contract package key")
        .into_entity_hash_addr()
        .unwrap_or_revert();

    runtime::call_versioned_contract::<()>(
        contract_package_hash.into(),
        Some(ENTITY_INITIAL_VERSION),
        SESSION_CODE,
        runtime_args! {},
    );
}

#[no_mangle]
pub extern "C" fn add_new_key() {
    let uref = storage::new_uref(());
    runtime::put_key(NEW_KEY, uref.into());
}

#[no_mangle]
pub extern "C" fn add_new_key_as_session() {
    let contract_package_hash = runtime::get_key(PACKAGE_HASH_KEY)
        .expect("should have package hash")
        .into_entity_hash_addr()
        .unwrap_or_revert()
        .into();

    assert!(runtime::get_key(NEW_KEY).is_none());
    runtime::call_versioned_contract::<()>(
        contract_package_hash,
        Some(ENTITY_INITIAL_VERSION),
        "add_new_key",
        runtime_args! {},
    );
    assert!(runtime::get_key(NEW_KEY).is_some());
}

#[no_mangle]
pub extern "C" fn session_code_caller_as_contract() {
    let contract_package_key: Key = runtime::get_named_arg(PACKAGE_HASH_KEY);
    let contract_package_hash = contract_package_key.into_package_hash().unwrap_or_revert();
    runtime::call_versioned_contract::<()>(
        contract_package_hash,
        Some(ENTITY_INITIAL_VERSION),
        SESSION_CODE,
        runtime_args! {},
    );
}

fn create_entrypoints_1() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    let contract_code_test = EntryPoint::new(
        CONTRACT_CODE.to_string(),
        Vec::new(),
        CLType::I32,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(contract_code_test);

    let session_code_caller_as_contract = EntryPoint::new(
        "session_code_caller_as_contract".to_string(),
        Vec::new(),
        CLType::I32,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(session_code_caller_as_contract);

    entry_points
}

fn install_version_1(package_hash: PackageHash) -> (AddressableEntityHash, EntityVersion) {
    let contract_named_keys = {
        let contract_variable = storage::new_uref(0);

        let mut named_keys = NamedKeys::new();
        named_keys.insert("contract_named_key".to_string(), contract_variable.into());
        named_keys
    };

    let entry_points = create_entrypoints_1();
    storage::add_contract_version(package_hash, entry_points, contract_named_keys)
}

#[no_mangle]
pub extern "C" fn call() {
    // Session contract
    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();

    runtime::put_key(PACKAGE_HASH_KEY, contract_package_hash.into());
    runtime::put_key(PACKAGE_ACCESS_KEY, access_uref.into());
    let (contract_hash, contract_version) = install_version_1(contract_package_hash);
    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(CONTRACT_HASH_KEY, Key::contract_entity_key(contract_hash));
}
