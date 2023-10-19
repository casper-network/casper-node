#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::{contract_api::runtime, unwrap_or_revert::UnwrapOrRevert};

use casper_types::contract_messages::MessageTopicOperation;

const TOPIC_NAME: &str = "messages_topic";

#[no_mangle]
pub extern "C" fn call() {
    runtime::manage_message_topic(TOPIC_NAME, MessageTopicOperation::Add).unwrap_or_revert();
}
