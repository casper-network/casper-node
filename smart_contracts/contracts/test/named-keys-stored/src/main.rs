#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::ToString;

use casper_contract::{
    self,
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    addressable_entity::{NamedKeys, Parameters},
    ApiError, CLType, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key, PackageHash,
    RuntimeArgs,
};

const ENTRY_POINT_CONTRACT: &str = "named_keys_contract";
const ENTRY_POINT_CONTRACT_TO_CONTRACT: &str = "named_keys_contract_to_contract";
const ENTRY_POINT_SESSION_TO_SESSION: &str = "named_keys_session_to_session";
const ENTRY_POINT_SESSION: &str = "named_keys_session";
const CONTRACT_PACKAGE_HASH_NAME: &str = "contract_package_stored";
const CONTRACT_HASH_NAME: &str = "contract_stored";
const CONTRACT_VERSION: &str = "contract_version";

#[repr(u16)]
enum Error {
    HasWrongNamedKeys,
    FoundNamedKey1,
    FoundNamedKey2,
    FoundNamedKey3,
    FoundNamedKey4,
    UnexpectedContractValidURef,
    UnexpectedAccountValidURef,
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        ApiError::User(error as u16)
    }
}

#[no_mangle]
pub extern "C" fn named_keys_contract() {
    if runtime::get_key("account_named_key_1").is_some()
        || runtime::get_key("account_named_key_2").is_some()
        || runtime::get_key("account_named_key_3").is_some()
        || runtime::get_key("account_named_key_4").is_some()
    {
        runtime::revert(Error::HasWrongNamedKeys);
    }

    if runtime::get_key("named_key_1").is_none() {
        runtime::revert(Error::FoundNamedKey1);
    }
    if runtime::get_key("named_key_2").is_none() {
        runtime::revert(Error::FoundNamedKey2);
    }
    if runtime::get_key("named_key_3").is_none() {
        runtime::revert(Error::FoundNamedKey3);
    }
    let uref_key = runtime::get_key("named_key_4").unwrap_or_revert_with(Error::FoundNamedKey4);
    let uref = uref_key.into_uref().unwrap();
    if !runtime::is_valid_uref(uref) {
        runtime::revert(Error::UnexpectedContractValidURef);
    }
}

#[no_mangle]
pub extern "C" fn named_keys_session() {
    if runtime::get_key("named_key_1").is_some()
        || runtime::get_key("named_key_2").is_some()
        || runtime::get_key("named_key_3").is_some()
        || runtime::get_key("named_key_4").is_some()
    {
        runtime::revert(Error::HasWrongNamedKeys);
    }

    if runtime::get_key("account_named_key_1").is_none() {
        runtime::revert(Error::FoundNamedKey1);
    }
    if runtime::get_key("account_named_key_2").is_none() {
        runtime::revert(Error::FoundNamedKey2);
    }
    if runtime::get_key("account_named_key_3").is_none() {
        runtime::revert(Error::FoundNamedKey3);
    }
    if runtime::get_key("account_named_key_4").is_none() {
        runtime::revert(Error::FoundNamedKey4);
    }
    let uref_key = runtime::get_key("account_named_key_4")
        .unwrap_or_revert_with(Error::UnexpectedContractValidURef);
    let uref = uref_key.into_uref().unwrap();
    if !runtime::is_valid_uref(uref) {
        runtime::revert(Error::UnexpectedAccountValidURef);
    }
}

#[no_mangle]
pub extern "C" fn named_keys_contract_to_contract() {
    let package_hash = runtime::get_key(CONTRACT_PACKAGE_HASH_NAME)
        .and_then(Key::into_package_addr)
        .map(PackageHash::new)
        .unwrap_or_revert();

    runtime::call_versioned_contract::<()>(
        package_hash,
        None,
        ENTRY_POINT_CONTRACT,
        RuntimeArgs::default(),
    );
}

#[no_mangle]
pub extern "C" fn named_keys_session_to_session() {
    let package_hash = runtime::get_key(CONTRACT_PACKAGE_HASH_NAME)
        .and_then(Key::into_package_addr)
        .map(PackageHash::new)
        .unwrap_or_revert();

    runtime::call_versioned_contract::<()>(
        package_hash,
        None,
        ENTRY_POINT_SESSION,
        RuntimeArgs::default(),
    );
}

#[no_mangle]
pub extern "C" fn call() {
    runtime::put_key("account_named_key_1", Key::Hash([10; 32]));
    runtime::put_key("account_named_key_2", Key::Hash([11; 32]));
    runtime::put_key("account_named_key_3", Key::Hash([12; 32]));
    let uref = storage::new_uref(());
    runtime::put_key("account_named_key_4", Key::from(uref));

    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let contract_entrypoint = EntryPoint::new(
            ENTRY_POINT_CONTRACT.to_string(),
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entry_points.add_entry_point(contract_entrypoint);
        let session_entrypoint = EntryPoint::new(
            ENTRY_POINT_SESSION.to_string(),
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entry_points.add_entry_point(session_entrypoint);
        let contract_to_contract_entrypoint = EntryPoint::new(
            ENTRY_POINT_CONTRACT_TO_CONTRACT.to_string(),
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entry_points.add_entry_point(contract_to_contract_entrypoint);
        let contract_to_contract_entrypoint = EntryPoint::new(
            ENTRY_POINT_SESSION_TO_SESSION.to_string(),
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entry_points.add_entry_point(contract_to_contract_entrypoint);
        entry_points
    };

    let (contract_package_hash, _access) = storage::create_contract_package_at_hash();

    let named_keys = {
        let mut named_keys = NamedKeys::new();
        named_keys.insert("named_key_1".to_string(), Key::Hash([1; 32]));
        named_keys.insert("named_key_2".to_string(), Key::Hash([2; 32]));
        named_keys.insert("named_key_3".to_string(), Key::Hash([3; 32]));
        let uref = storage::new_uref(());
        named_keys.insert("named_key_4".to_string(), Key::from(uref));
        named_keys.insert(
            CONTRACT_PACKAGE_HASH_NAME.to_string(),
            Key::from(contract_package_hash),
        );
        named_keys
    };

    let (contract_hash, contract_version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);

    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(CONTRACT_PACKAGE_HASH_NAME, contract_package_hash.into());
    runtime::put_key(CONTRACT_HASH_NAME, Key::contract_entity_key(contract_hash));
}
