#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};

use casper_types::{
    addressable_entity::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, NamedKeys},
    api_error::ApiError,
    contract_messages::{MessagePayload, MessageTopicOperation},
    CLType, CLTyped, Parameter, RuntimeArgs,
};

pub const ENTRY_POINT_INIT: &str = "init";
pub const ENTRY_POINT_EMIT_MESSAGE: &str = "emit_message";
pub const MESSAGE_EMITTER_INITIALIZED: &str = "message_emitter_initialized";
pub const ARG_MESSAGE_SUFFIX_NAME: &str = "message_suffix";

pub const MESSAGE_EMITTER_GENERIC_TOPIC: &str = "generic_messages";
pub const MESSAGE_PREFIX: &str = "generic message: ";

#[no_mangle]
pub extern "C" fn emit_message() {
    let suffix: String = runtime::get_named_arg(ARG_MESSAGE_SUFFIX_NAME);

    runtime::emit_message(
        MESSAGE_EMITTER_GENERIC_TOPIC,
        &MessagePayload::from_string(format!("{}{}", MESSAGE_PREFIX, suffix)),
    );
}

#[no_mangle]
pub extern "C" fn init() {
    if runtime::has_key(MESSAGE_EMITTER_INITIALIZED) {
        runtime::revert(ApiError::User(0));
    }

    runtime::manage_message_topic(MESSAGE_EMITTER_GENERIC_TOPIC, MessageTopicOperation::Add);

    runtime::put_key(MESSAGE_EMITTER_INITIALIZED, storage::new_uref(()).into());
}

#[no_mangle]
pub extern "C" fn call() {
    let mut emitter_entry_points = EntryPoints::new();
    emitter_entry_points.add_entry_point(EntryPoint::new(
        ENTRY_POINT_INIT,
        Vec::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    emitter_entry_points.add_entry_point(EntryPoint::new(
        ENTRY_POINT_EMIT_MESSAGE,
        vec![Parameter::new(ARG_MESSAGE_SUFFIX_NAME, String::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));

    let (stored_contract_hash, _contract_version) = storage::new_contract(
        emitter_entry_points,
        Some(NamedKeys::new()),
        Some("messages_emitter_package_name".to_string()),
        Some("messages_emitter_access_uref".to_string()),
    );

    // Call contract to initialize it
    runtime::call_contract::<()>(
        stored_contract_hash,
        ENTRY_POINT_INIT,
        RuntimeArgs::default(),
    );
}
