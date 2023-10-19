#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    addressable_entity::{EntryPoint, EntryPoints, NamedKeys, Parameters},
    bytesrepr::FromBytes,
    ApiError, CLType, CLTyped, EntryPointAccess, EntryPointType, Key, URef, U512,
};

const ACCESS_KEY_NAME: &str = "factory_access";
const ARG_INITIAL_VALUE: &str = "initial_value";
const ARG_NAME: &str = "name";
const CONTRACT_FACTORY_DEFAULT_ENTRY_POINT: &str = "contract_factory_default";
const CONTRACT_FACTORY_ENTRY_POINT: &str = "contract_factory";
const CONTRACT_VERSION: &str = "contract_version";
const CURRENT_VALUE_KEY: &str = "current_value";
const DECREASE_ENTRY_POINT: &str = "decrement";
const HASH_KEY_NAME: &str = "factory_hash";
const INCREASE_ENTRY_POINT: &str = "increment";
const PACKAGE_HASH_KEY_NAME: &str = "factory_package_hash";

fn get_named_uref(name: &str) -> Result<URef, ApiError> {
    runtime::get_key(name)
        .ok_or(ApiError::MissingKey)?
        .into_uref()
        .ok_or(ApiError::UnexpectedKeyVariant)
}

fn read_uref<T: CLTyped + FromBytes>(uref: URef) -> Result<T, ApiError> {
    let value: T = storage::read(uref)?.ok_or(ApiError::ValueNotFound)?;
    Ok(value)
}

fn modify_counter(func: impl FnOnce(U512) -> U512) -> Result<(), ApiError> {
    let current_value_uref = get_named_uref(CURRENT_VALUE_KEY)?;
    let value: U512 = read_uref(current_value_uref)?;
    let new_value = func(value);
    storage::write(current_value_uref, new_value);
    Ok(())
}

#[no_mangle]
pub extern "C" fn increment() {
    modify_counter(|value| value + U512::one()).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn decrement() {
    modify_counter(|value| value - U512::one()).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn contract_factory() {
    let name: String = runtime::get_named_arg(ARG_NAME);
    let initial_value: U512 = runtime::get_named_arg(ARG_INITIAL_VALUE);
    installer(name, initial_value);
}

#[no_mangle]
pub extern "C" fn contract_factory_default() {
    let name: String = runtime::get_named_arg(ARG_NAME);
    installer(name, U512::zero());
}

fn installer(name: String, initial_value: U512) {
    let named_keys = {
        let new_uref = storage::new_uref(initial_value);
        let mut named_keys = NamedKeys::new();
        named_keys.insert(CURRENT_VALUE_KEY.to_string(), new_uref.into());
        named_keys
    };

    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let entry_point: EntryPoint = EntryPoint::new(
            INCREASE_ENTRY_POINT.to_string(),
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point: EntryPoint = EntryPoint::new(
            DECREASE_ENTRY_POINT.to_string(),
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entry_points.add_entry_point(entry_point);

        entry_points
    };

    let (contract_hash, contract_version) = storage::new_contract(
        entry_points,
        Some(named_keys),
        Some(PACKAGE_HASH_KEY_NAME.to_string()),
        Some(ACCESS_KEY_NAME.to_string()),
    );

    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(&name, Key::contract_entity_key(contract_hash));
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point: EntryPoint = EntryPoint::new(
            CONTRACT_FACTORY_ENTRY_POINT.to_string(),
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Factory,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point: EntryPoint = EntryPoint::new(
            CONTRACT_FACTORY_DEFAULT_ENTRY_POINT.to_string(),
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Factory,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point: EntryPoint = EntryPoint::new(
            INCREASE_ENTRY_POINT.to_string(),
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Template,
            EntryPointType::AddressableEntity,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point: EntryPoint = EntryPoint::new(
            DECREASE_ENTRY_POINT.to_string(),
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Template,
            EntryPointType::AddressableEntity,
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
    runtime::put_key(HASH_KEY_NAME, Key::contract_entity_key(contract_hash));
}
