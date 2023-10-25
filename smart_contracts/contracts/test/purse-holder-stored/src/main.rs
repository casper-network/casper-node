#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::ToString, vec};

use alloc::string::String;
use casper_contract::{
    self,
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    CLType, CLValue, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key, Parameter,
};

pub const METHOD_ADD: &str = "add";
pub const METHOD_REMOVE: &str = "remove";
pub const METHOD_VERSION: &str = "version";

const ENTRY_POINT_ADD: &str = "add_named_purse";
const ENTRY_POINT_VERSION: &str = "version";
const HASH_KEY_NAME: &str = "purse_holder";
const ACCESS_KEY_NAME: &str = "purse_holder_access";
const ARG_PURSE: &str = "purse_name";
const ARG_IS_LOCKED: &str = "is_locked";
const VERSION: &str = "1.0.0";
const PURSE_HOLDER_STORED_CONTRACT_NAME: &str = "purse_holder_stored";
const CONTRACT_VERSION: &str = "contract_version";

#[no_mangle]
pub extern "C" fn add_named_purse() {
    let purse_name: String = runtime::get_named_arg(ARG_PURSE);
    let purse = system::create_purse();
    runtime::put_key(&purse_name, purse.into());
}

#[no_mangle]
pub extern "C" fn version() {
    let ret = CLValue::from_t(VERSION).unwrap_or_revert();
    runtime::ret(ret);
}

#[no_mangle]
pub extern "C" fn call() {
    let is_locked: bool = runtime::get_named_arg(ARG_IS_LOCKED);
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let add = EntryPoint::new(
            ENTRY_POINT_ADD.to_string(),
            vec![Parameter::new(ARG_PURSE, CLType::String)],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entry_points.add_entry_point(add);
        let version = EntryPoint::new(
            ENTRY_POINT_VERSION.to_string(),
            vec![],
            CLType::String,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entry_points.add_entry_point(version);
        entry_points
    };

    let (contract_hash, contract_version) = if !is_locked {
        storage::new_contract(
            entry_points,
            None,
            Some(HASH_KEY_NAME.to_string()),
            Some(ACCESS_KEY_NAME.to_string()),
        )
    } else {
        storage::new_locked_contract(
            entry_points,
            None,
            Some(HASH_KEY_NAME.to_string()),
            Some(ACCESS_KEY_NAME.to_string()),
        )
    };

    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(
        PURSE_HOLDER_STORED_CONTRACT_NAME,
        Key::contract_entity_key(contract_hash),
    );
    runtime::put_key(ENTRY_POINT_VERSION, storage::new_uref(VERSION).into());
}
