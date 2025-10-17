use bytes::Bytes;
use casper_executor_wasm_common::error::{
    HOST_ERROR_INVALID_INPUT, HOST_ERROR_MAX_MESSAGES_PER_BLOCK_EXCEEDED,
    HOST_ERROR_MESSAGE_TOPIC_FULL, HOST_ERROR_PAYLOAD_TOO_LONG, HOST_ERROR_SUCCESS,
    HOST_ERROR_TOO_MANY_TOPICS, HOST_ERROR_TOPIC_TOO_LONG,
};
use casper_executor_wasm_interface::{Caller, FatalHostError, VMError, VMResult};
use casper_storage::{global_state::GlobalStateReader, tracking_copy::TrackingCopyExt};
use casper_types::{
    addressable_entity::MessageTopicError,
    bytesrepr::{self, Bytes as BytesreprBytes, ToBytes},
    contract_messages::{Message, MessageAddr, MessagePayload, MessageTopicSummary},
    BlockGlobalAddr, BlockTime, CLValue, Digest, EntityAddr, Key, StoredValue,
};
use tracing::trace;

use crate::{context::Context, host::charge_gas_storage};

pub(crate) fn print_std(input: Bytes) -> VMResult<u32> {
    let msg = String::from_utf8_lossy(&input);
    eprintln!("⛓️ {msg}");
    Ok(HOST_ERROR_SUCCESS)
}

pub(crate) fn emit<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
    input: Bytes,
) -> VMResult<u32> {
    let (topic_name, payload) =
        match bytesrepr::deserialize_from_slice::<&Bytes, (String, BytesreprBytes)>(&input) {
            Ok(res) => res,
            Err(_) => {
                return Ok(HOST_ERROR_INVALID_INPUT);
            }
        };
    let message_limits = caller.context().message_limits;
    if topic_name.len() > (message_limits.max_topic_name_size as usize) {
        return Ok(HOST_ERROR_TOPIC_TOO_LONG);
    }

    if payload.len() > (message_limits.max_message_size as usize) {
        return Ok(HOST_ERROR_PAYLOAD_TOO_LONG);
    }

    let entity_addr = context_to_entity_addr(&caller.context().callee);

    let mut message_topics = caller
        .context_mut()
        .tracking_copy
        .get_message_topics(entity_addr)
        .unwrap_or_else(|error| {
            panic!("Error while reading from storage; aborting error={error:?}")
        });

    if message_topics.len() >= message_limits.max_topics_per_contract as usize {
        return Ok(HOST_ERROR_TOO_MANY_TOPICS);
    }

    let topic_name_hash = Digest::hash(&topic_name).value().into();

    match message_topics.add_topic(&topic_name, topic_name_hash) {
        Ok(()) => {
            // New topic is created
        }
        Err(MessageTopicError::DuplicateTopic) => {
            // We're lazily creating message topics and this operation is idempotent.
            // Therefore, already existing topic is not an issue.
        }
        Err(MessageTopicError::MaxTopicsExceeded) => {
            // We're validating the size of topics before adding them
            return Ok(HOST_ERROR_TOO_MANY_TOPICS);
        }
        Err(MessageTopicError::TopicNameSizeExceeded) => {
            // We're validating the length of topic before adding it
            return Ok(HOST_ERROR_TOPIC_TOO_LONG);
        }
        Err(error) => {
            // These error variants are non_exhaustive, and we should handle them explicitly.
            unreachable!("Unexpected error while adding a topic: {:?}", error);
        }
    };

    let current_block_time = caller.context().block_time;
    trace!("📩 {topic_name}: {payload:?} (at {current_block_time:?})");

    let topic_key = Key::Message(MessageAddr::new_topic_addr(entity_addr, topic_name_hash));
    let prev_topic_summary = match caller.context_mut().tracking_copy.read(&topic_key) {
        Ok(Some(StoredValue::MessageTopic(message_topic_summary))) => message_topic_summary,
        Ok(Some(stored_value)) => {
            panic!("Unexpected stored value: {stored_value:?}");
        }
        Ok(None) => {
            let message_topic_summary =
                MessageTopicSummary::new(0, current_block_time, topic_name.clone());
            let summary = StoredValue::MessageTopic(message_topic_summary.clone());
            caller.context_mut().tracking_copy.write(topic_key, summary);
            message_topic_summary
        }
        Err(error) => panic!("Error while reading from storage; aborting error={error:?}"),
    };

    let topic_message_index = if prev_topic_summary.blocktime() != current_block_time {
        for index in 1..prev_topic_summary.message_count() {
            let message_key = Key::message(entity_addr, topic_name_hash, index);
            debug_assert!(
                {
                    // NOTE: This assertion is to ensure that the message index is continuous, and
                    // the previous messages are pruned properly.
                    caller
                        .context_mut()
                        .tracking_copy
                        .read(&message_key)
                        .map_err(|_| VMError::Fatal(FatalHostError::TrackingCopy))?
                        .is_some()
                },
                "Message index is not continuous"
            );

            // Prune the previous messages
            caller.context_mut().tracking_copy.prune(message_key);
        }
        0
    } else {
        prev_topic_summary.message_count()
    };

    // Data stored in the global state associated with the message block.
    type MessageCountPair = (BlockTime, u64);

    let block_message_index: u64 = match caller
        .context_mut()
        .tracking_copy
        .read(&Key::BlockGlobal(BlockGlobalAddr::MessageCount))
    {
        Ok(Some(StoredValue::CLValue(value_pair))) => {
            let (prev_block_time, prev_count): MessageCountPair =
                CLValue::into_t(value_pair).map_err(|_| FatalHostError::TypeConversion)?;
            if prev_block_time == current_block_time {
                prev_count
            } else {
                0
            }
        }
        Ok(Some(other)) => panic!("Unexpected stored value: {other:?}"),
        Ok(None) => {
            // No messages in current block yet
            0
        }
        Err(error) => {
            panic!("Error while reading from storage; aborting error={error:?}")
        }
    };

    let Some(topic_message_count) = topic_message_index.checked_add(1) else {
        return Ok(HOST_ERROR_MESSAGE_TOPIC_FULL);
    };

    let Some(block_message_count) = block_message_index.checked_add(1) else {
        return Ok(HOST_ERROR_MAX_MESSAGES_PER_BLOCK_EXCEEDED);
    };

    // Under v2 runtime messages are only limited to bytes.
    let message_payload = MessagePayload::Bytes(payload);

    let message = Message::new(
        entity_addr,
        message_payload,
        topic_name,
        topic_name_hash,
        topic_message_index,
        block_message_index,
    );
    let topic_value = StoredValue::MessageTopic(MessageTopicSummary::new(
        topic_message_count,
        current_block_time,
        message.topic_name().to_owned(),
    ));

    let message_key = message.message_key();
    let message_value = StoredValue::Message(
        message
            .checksum()
            .map_err(|_| FatalHostError::MessageChecksumMissing)?,
    );
    let message_count_pair: MessageCountPair = (current_block_time, block_message_count);
    let block_message_count_value = StoredValue::CLValue(
        CLValue::from_t(message_count_pair).map_err(|_| FatalHostError::TypeConversion)?,
    );

    // Charge for amount as measured by serialized length
    let bytes_count = topic_value.serialized_length()
        + message_value.serialized_length()
        + block_message_count_value.serialized_length();
    charge_gas_storage(caller, bytes_count)?;

    caller.context_mut().tracking_copy.emit_message(
        topic_key,
        topic_value,
        message_key,
        message_value,
        block_message_count_value,
        message,
    );

    Ok(HOST_ERROR_SUCCESS)
}

fn context_to_entity_addr(callee: &Key) -> EntityAddr {
    match callee {
        Key::Account(account_hash) => EntityAddr::new_account(account_hash.value()),
        Key::Hash(hash_addr) => EntityAddr::SmartContract(*hash_addr),
        Key::AddressableEntity(smart_contract_addr) => *smart_contract_addr,
        _ => {
            // This should never happen, as the caller is always an account or a smart contract.
            panic!("Unexpected callee variant: {:?}", callee)
        }
    }
}
