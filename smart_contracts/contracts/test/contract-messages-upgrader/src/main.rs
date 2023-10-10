#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;
use alloc::{string::String, vec::Vec};

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};

use casper_types::{
    addressable_entity::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, NamedKeys},
    api_error::ApiError,
    contract_messages::{MessagePayload, MessageTopicOperation},
    CLType, CLTyped, ContractPackageHash, Parameter, RuntimeArgs,
};

const ENTRY_POINT_INIT: &str = "init";
const ENTRY_POINT_EMIT_MESSAGE: &str = "upgraded_emit_message";
const UPGRADED_MESSAGE_EMITTER_INITIALIZED: &str = "upgraded_message_emitter_initialized";
const ARG_MESSAGE_SUFFIX_NAME: &str = "message_suffix";
const PACKAGE_HASH_KEY_NAME: &str = "messages_emitter_package_hash";

pub const MESSAGE_EMITTER_GENERIC_TOPIC: &str = "new_topic_after_upgrade";
pub const MESSAGE_PREFIX: &str = "generic message: ";

#[no_mangle]
pub extern "C" fn upgraded_emit_message() {
    let suffix: String = runtime::get_named_arg(ARG_MESSAGE_SUFFIX_NAME);

    runtime::emit_message(
        MESSAGE_EMITTER_GENERIC_TOPIC,
        &MessagePayload::from_string(format!("{}{}", MESSAGE_PREFIX, suffix)),
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

    let message_emitter_package_hash: ContractPackageHash = runtime::get_key(PACKAGE_HASH_KEY_NAME)
        .unwrap_or_revert()
        .into_hash()
        .unwrap()
        .into();

    let (contract_hash, _contract_version) = storage::add_contract_version(
        message_emitter_package_hash,
        emitter_entry_points,
        NamedKeys::new(),
    );

    // Call contract to initialize it
    runtime::call_contract::<()>(contract_hash, ENTRY_POINT_INIT, RuntimeArgs::default());
}
