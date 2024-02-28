#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;
use alloc::{
    collections::BTreeMap,
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
    contract_messages::MessageTopicOperation,
    CLType, CLTyped, Parameter, RuntimeArgs,
};

const ENTRY_POINT_INIT: &str = "init";
const ENTRY_POINT_EMIT_MESSAGE: &str = "emit_message";
const ENTRY_POINT_EMIT_MULTIPLE_MESSAGES: &str = "emit_multiple_messages";
const ENTRY_POINT_ADD_TOPIC: &str = "add_topic";
const MESSAGE_EMITTER_INITIALIZED: &str = "message_emitter_initialized";
const ARG_MESSAGE_SUFFIX_NAME: &str = "message_suffix";
const ARG_NUM_MESSAGES_TO_EMIT: &str = "num_messages_to_emit";
const ARG_REGISTER_DEFAULT_TOPIC_WITH_INIT: &str = "register_default_topic_with_init";
const ARG_TOPIC_NAME: &str = "topic_name";
const PACKAGE_HASH_KEY_NAME: &str = "messages_emitter_package_hash";
const ACCESS_KEY_NAME: &str = "messages_emitter_access";

pub const MESSAGE_EMITTER_GENERIC_TOPIC: &str = "generic_messages";
pub const MESSAGE_PREFIX: &str = "generic message: ";

#[no_mangle]
pub extern "C" fn emit_message() {
    let suffix: String = runtime::get_named_arg(ARG_MESSAGE_SUFFIX_NAME);

    runtime::emit_message(
        MESSAGE_EMITTER_GENERIC_TOPIC,
        &format!("{}{}", MESSAGE_PREFIX, suffix).into(),
    )
    .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn emit_multiple_messages() {
    let num_messages: u32 = runtime::get_named_arg(ARG_NUM_MESSAGES_TO_EMIT);

    for i in 0..num_messages {
        runtime::emit_message(
            MESSAGE_EMITTER_GENERIC_TOPIC,
            &format!("{}{}", MESSAGE_PREFIX, i).into(),
        )
        .unwrap_or_revert();
    }
}

#[no_mangle]
pub extern "C" fn add_topic() {
    let topic_name: String = runtime::get_named_arg(ARG_TOPIC_NAME);

    runtime::manage_message_topic(topic_name.as_str(), MessageTopicOperation::Add)
        .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn init() {
    if runtime::has_key(MESSAGE_EMITTER_INITIALIZED) {
        runtime::revert(ApiError::User(0));
    }

    runtime::manage_message_topic(MESSAGE_EMITTER_GENERIC_TOPIC, MessageTopicOperation::Add)
        .unwrap_or_revert();

    runtime::put_key(MESSAGE_EMITTER_INITIALIZED, storage::new_uref(()).into());
}

#[no_mangle]
pub extern "C" fn call() {
    let register_topic_with_init: bool =
        runtime::get_named_arg(ARG_REGISTER_DEFAULT_TOPIC_WITH_INIT);

    let mut emitter_entry_points = EntryPoints::new();
    emitter_entry_points.add_entry_point(EntryPoint::new(
        ENTRY_POINT_INIT,
        Vec::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    ));
    emitter_entry_points.add_entry_point(EntryPoint::new(
        ENTRY_POINT_EMIT_MESSAGE,
        vec![Parameter::new(ARG_MESSAGE_SUFFIX_NAME, String::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    ));
    emitter_entry_points.add_entry_point(EntryPoint::new(
        ENTRY_POINT_ADD_TOPIC,
        vec![Parameter::new(ARG_TOPIC_NAME, String::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    ));
    emitter_entry_points.add_entry_point(EntryPoint::new(
        ENTRY_POINT_EMIT_MULTIPLE_MESSAGES,
        vec![Parameter::new(ARG_NUM_MESSAGES_TO_EMIT, u32::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    ));

    if register_topic_with_init {
        let (stored_contract_hash, _contract_version) = storage::new_contract(
            emitter_entry_points,
            Some(NamedKeys::new()),
            Some(PACKAGE_HASH_KEY_NAME.into()),
            Some(ACCESS_KEY_NAME.into()),
            None,
        );

        // Call contract to initialize it and register the default topic.
        runtime::call_contract::<()>(
            stored_contract_hash,
            ENTRY_POINT_INIT,
            RuntimeArgs::default(),
        );
    } else {
        let new_topics = BTreeMap::from([(
            MESSAGE_EMITTER_GENERIC_TOPIC.to_string(),
            MessageTopicOperation::Add,
        )]);
        // Register the default topic on contract creation and not through the initializer.
        let (_stored_contract_hash, _contract_version) = storage::new_contract(
            emitter_entry_points,
            Some(NamedKeys::new()),
            Some(PACKAGE_HASH_KEY_NAME.into()),
            Some(ACCESS_KEY_NAME.into()),
            Some(new_topics),
        );
    }
}
