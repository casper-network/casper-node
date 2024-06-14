#![no_main]
#![no_std]

extern crate alloc;

use alloc::{string::ToString, vec};

use casper_contract::{
    contract_api::{runtime, runtime::revert, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    ApiError, CLType, EntryPoint, EntryPointAccess, EntryPointPayment, EntryPointType, EntryPoints,
    Key,
};

const PACKAGE_NAME: &str = "load_caller_info_package";
const CONTRACT_HASH: &str = "load_caller_info_contract_hash";
const PACKAGE_ACCESS_KEY: &str = "package_access_key";

#[no_mangle]
pub extern "C" fn initiator() {
    let initiator = runtime::get_call_initiator().unwrap_or_revert();
    runtime::put_key("initiator", Key::URef(storage::new_uref(initiator)))
}

#[no_mangle]
pub extern "C" fn get_immediate_caller() {
    let initiator = runtime::get_immediate_caller().unwrap_or_revert();
    runtime::put_key("immediate", Key::URef(storage::new_uref(initiator)))
}

#[no_mangle]
pub extern "C" fn get_full_stack() {
    let initiator = runtime::get_call_stack();
    if initiator.is_empty() {
        revert(ApiError::User(10))
    }
    runtime::put_key("full", Key::URef(storage::new_uref(initiator)))
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let initiator_entry_point = EntryPoint::new(
            "initiator".to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Called,
            EntryPointPayment::Caller,
        );
        let immediate_entry_point = EntryPoint::new(
            "get_immediate_caller".to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Called,
            EntryPointPayment::Caller,
        );
        let full_stack_entry_point = EntryPoint::new(
            "get_full_stack".to_string(),
            vec![],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Called,
            EntryPointPayment::Caller,
        );
        entry_points.add_entry_point(initiator_entry_point);
        entry_points.add_entry_point(immediate_entry_point);
        entry_points.add_entry_point(full_stack_entry_point);
        entry_points
    };

    let (contract_hash, _contract_version) = storage::new_contract(
        entry_points,
        None,
        Some(PACKAGE_NAME.to_string()),
        Some(PACKAGE_ACCESS_KEY.to_string()),
        None,
    );

    runtime::put_key(CONTRACT_HASH, Key::contract_entity_key(contract_hash));
}
