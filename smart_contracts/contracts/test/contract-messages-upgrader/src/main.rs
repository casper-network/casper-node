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
    runtime_args, CLType, CLTyped, PackageHash, Parameter, RuntimeArgs,
};

const ENTRY_POINT_INIT: &str = "init";
const FIRST_VERSION_ENTRY_POINT_EMIT_MESSAGE: &str = "emit_message";
const ENTRY_POINT_EMIT_MESSAGE: &str = "upgraded_emit_message";
const ENTRY_POINT_EMIT_MESSAGE_FROM_EACH_VERSION: &str = "emit_message_from_each_version";
const UPGRADED_MESSAGE_EMITTER_INITIALIZED: &str = "upgraded_message_emitter_initialized";
const ARG_MESSAGE_SUFFIX_NAME: &str = "message_suffix";
const PACKAGE_HASH_KEY_NAME: &str = "messages_emitter_package_hash";
const ARG_REGISTER_DEFAULT_TOPIC_WITH_INIT: &str = "register_default_topic_with_init";

pub const MESSAGE_EMITTER_GENERIC_TOPIC: &str = "new_topic_after_upgrade";
pub const MESSAGE_PREFIX: &str = "generic message: ";

#[no_mangle]
pub extern "C" fn upgraded_emit_message() {
    let suffix: String = runtime::get_named_arg(ARG_MESSAGE_SUFFIX_NAME);

    runtime::emit_message(
        MESSAGE_EMITTER_GENERIC_TOPIC,
        &format!("{}{}", MESSAGE_PREFIX, suffix).into(),
    )
    .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn emit_message_from_each_version() {
    let suffix: String = runtime::get_named_arg(ARG_MESSAGE_SUFFIX_NAME);

    let contract_package_hash: PackageHash = runtime::get_key(PACKAGE_HASH_KEY_NAME)
        .expect("should have contract package key")
        .into_package_addr()
        .unwrap_or_revert()
        .into();

    // Emit a message from this contract.
    runtime::emit_message(
        MESSAGE_EMITTER_GENERIC_TOPIC,
        &"emitting multiple messages".into(),
    )
    .unwrap_or_revert();

    // Call previous contract version which will emit a message.
    runtime::call_versioned_contract::<()>(
        contract_package_hash,
        Some(1),
        FIRST_VERSION_ENTRY_POINT_EMIT_MESSAGE,
        runtime_args! {
            ARG_MESSAGE_SUFFIX_NAME => suffix.clone(),
        },
    );

    // Emit another message from this version.
    runtime::emit_message(
        MESSAGE_EMITTER_GENERIC_TOPIC,
        &format!("{}{}", MESSAGE_PREFIX, suffix).into(),
    )
    .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn init() {
    if runtime::has_key(UPGRADED_MESSAGE_EMITTER_INITIALIZED) {
        runtime::revert(ApiError::User(0));
    }

    runtime::manage_message_topic(MESSAGE_EMITTER_GENERIC_TOPIC, MessageTopicOperation::Add)
        .unwrap_or_revert();

    runtime::put_key(
        UPGRADED_MESSAGE_EMITTER_INITIALIZED,
        storage::new_uref(()).into(),
    );
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
        EntryPointType::Called,
    ));
    emitter_entry_points.add_entry_point(EntryPoint::new(
        ENTRY_POINT_EMIT_MESSAGE,
        vec![Parameter::new(ARG_MESSAGE_SUFFIX_NAME, String::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Called,
    ));
    emitter_entry_points.add_entry_point(EntryPoint::new(
        ENTRY_POINT_EMIT_MESSAGE_FROM_EACH_VERSION,
        vec![Parameter::new(ARG_MESSAGE_SUFFIX_NAME, String::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Called,
    ));

    let message_emitter_package_hash: PackageHash = runtime::get_key(PACKAGE_HASH_KEY_NAME)
        .unwrap_or_revert()
        .into_package_addr()
        .unwrap()
        .into();

    let mut named_keys = NamedKeys::new();
    named_keys.insert(
        PACKAGE_HASH_KEY_NAME.into(),
        message_emitter_package_hash.into(),
    );

    if register_topic_with_init {
        let (contract_hash, _contract_version) = storage::add_contract_version(
            message_emitter_package_hash,
            emitter_entry_points,
            named_keys,
            BTreeMap::new(),
        );

        // Call contract to initialize it
        runtime::call_contract::<()>(contract_hash, ENTRY_POINT_INIT, RuntimeArgs::default());
    } else {
        let new_topics = BTreeMap::from([(
            MESSAGE_EMITTER_GENERIC_TOPIC.to_string(),
            MessageTopicOperation::Add,
        )]);
        let (_contract_hash, _contract_version) = storage::add_contract_version(
            message_emitter_package_hash,
            emitter_entry_points,
            named_keys,
            new_topics,
        );
    }
}
