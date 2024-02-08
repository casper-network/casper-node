#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, string::ToString, vec};

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    CLType, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key, Parameter,
};

use get_call_stack_recursive_subcall::{
    ARG_CALLS, ARG_CURRENT_DEPTH, CONTRACT_NAME, CONTRACT_PACKAGE_NAME,
    METHOD_FORWARDER_CONTRACT_NAME, METHOD_FORWARDER_SESSION_NAME, PACKAGE_ACCESS_KEY_NAME,
};

#[no_mangle]
pub extern "C" fn forwarder_contract() {
    get_call_stack_recursive_subcall::recurse()
}

#[no_mangle]
pub extern "C" fn forwarder_session() {
    get_call_stack_recursive_subcall::recurse()
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let forwarder_contract_entry_point = EntryPoint::new(
            METHOD_FORWARDER_CONTRACT_NAME.to_string(),
            vec![
                Parameter::new(ARG_CALLS, CLType::List(Box::new(CLType::Any))),
                Parameter::new(ARG_CURRENT_DEPTH, CLType::U8),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        let forwarder_session_entry_point = EntryPoint::new(
            METHOD_FORWARDER_SESSION_NAME.to_string(),
            vec![
                Parameter::new(ARG_CALLS, CLType::List(Box::new(CLType::Any))),
                Parameter::new(ARG_CURRENT_DEPTH, CLType::U8),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );
        entry_points.add_entry_point(forwarder_contract_entry_point);
        entry_points.add_entry_point(forwarder_session_entry_point);
        entry_points
    };

    let (contract_hash, _contract_version) = storage::new_contract(
        entry_points,
        None,
        Some(CONTRACT_PACKAGE_NAME.to_string()),
        Some(PACKAGE_ACCESS_KEY_NAME.to_string()),
    );

    runtime::put_key(CONTRACT_NAME, Key::contract_entity_key(contract_hash));
}
