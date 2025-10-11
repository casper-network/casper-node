use std::convert::TryFrom;

use crate::{
    global_state::{error::Error, state::StateReader},
    system::protocol_upgrade::blake2b,
    tracking_copy::TrackingCopyError,
    TrackingCopy,
};

use casper_types::{
    bytesrepr,
    contract_messages::{Message, MessageAddr, MessagePayload, MessageTopicSummary},
    BlockGlobalAddr, BlockTime, CLValue, CLValueError, EntityAddr, Key, PublicKey, StoredValue,
    StoredValueTypeMismatch,
};
use thiserror::Error;

const MESSAGING_PACKAGE_ADDR_TOPIC: &str = "package_key";
const MESSAGING_CONTRACT_ADDR_TOPIC: &str = "contract_key";
const MESSAGING_CONTRACT_BYTECODE_ADDR_TOPIC: &str = "bytecode_key";

const MESSAGING_CONTRACT_VERSION_TOPIC: &str = "contract_version";

/// Errors that can be returned by the code emitting messages for a new contract version
#[derive(Debug, Error)]
pub enum MessageEmissionError {
    /// Error occured when trying to type downcast CLValue
    #[error("Error occured when trying to type downcast CLValue: {0}")]
    CLValue(CLValueError),
    /// Error when fetching data from the tracking copy
    #[error("Error when fetching data from the tracking copy: {0}")]
    TrackingCopy(TrackingCopyError),
    /// Error when casting types
    #[error("Error when casting types: {0}")]
    TypeMismatch(StoredValueTypeMismatch),
    /// Error when formatting a data structure in binary format
    #[error("Error when formatting a data structure in binary format: {0}")]
    BytesRepr(bytesrepr::Error),
    /// Given topic was requested but does not exist
    #[error("Given topic was requested but does not exist: {0}")]
    TopicNotRegistered(Key),
    /// Given topic is full
    #[error("Given topic is full: {0}")]
    TopicFull(Key),
    /// No more messages in block allowed
    #[error(" No more messages in block allowed")]
    MaxMessagesPerBlockExceeded,
}

impl From<TrackingCopyError> for MessageEmissionError {
    fn from(err: TrackingCopyError) -> Self {
        MessageEmissionError::TrackingCopy(err)
    }
}

impl From<CLValueError> for MessageEmissionError {
    fn from(err: CLValueError) -> Self {
        MessageEmissionError::CLValue(err)
    }
}

struct MessageEmitter {}
impl MessageEmitter {
    /// This function should be used ONLY for emitting
    /// system messages that don't charge gas since the
    /// mechanism doesn't account gas cost.
    fn emit_message_for_entity<T>(
        &self,
        tracking_copy: &mut TrackingCopy<T>,
        entity_addr: EntityAddr,
        topic_name: &str,
        message_payload: MessagePayload,
        current_blocktime: BlockTime,
    ) -> Result<(), MessageEmissionError>
    where
        T: StateReader<Key, StoredValue, Error = Error>,
    {
        let topic_name_hash = blake2b(topic_name).into();
        let topic_key = Key::Message(MessageAddr::new_topic_addr(entity_addr, topic_name_hash));
        // Check if the topic exists and get the summary.
        let Some(StoredValue::MessageTopic(prev_topic_summary)) = tracking_copy
            .read(&topic_key)
            .map_err(MessageEmissionError::TrackingCopy)?
        else {
            return Err(MessageEmissionError::TopicNotRegistered(topic_key));
        };

        let topic_message_index = if prev_topic_summary.blocktime() != current_blocktime {
            for index in 1..prev_topic_summary.message_count() {
                tracking_copy.prune(Key::message(entity_addr, topic_name_hash, index));
            }
            0
        } else {
            prev_topic_summary.message_count()
        };

        let block_message_index: u64 = match tracking_copy
            .read(&Key::BlockGlobal(BlockGlobalAddr::MessageCount))?
        {
            Some(stored_value) => {
                let (prev_block_time, prev_count): (BlockTime, u64) = CLValue::into_t(
                    CLValue::try_from(stored_value).map_err(MessageEmissionError::TypeMismatch)?,
                )
                .map_err(MessageEmissionError::CLValue)?;
                if prev_block_time == current_blocktime {
                    prev_count
                } else {
                    0
                }
            }
            None => 0,
        };

        let Some(topic_message_count) = topic_message_index.checked_add(1) else {
            return Err(MessageEmissionError::TopicFull(topic_key));
        };

        let Some(block_message_count) = block_message_index.checked_add(1) else {
            return Err(MessageEmissionError::MaxMessagesPerBlockExceeded);
        };
        let message = Message::new(
            entity_addr,
            message_payload,
            topic_name.to_string(),
            topic_name_hash,
            topic_message_index,
            block_message_index,
        );
        let topic_value = StoredValue::MessageTopic(MessageTopicSummary::new(
            topic_message_count,
            current_blocktime,
            message.topic_name().to_owned(),
        ));
        let message_key = message.message_key();
        let message_value = StoredValue::Message(
            message
                .checksum()
                .map_err(MessageEmissionError::BytesRepr)?,
        );
        let cl_value = CLValue::from_t((current_blocktime, block_message_count))?;
        let block_message_count_value = StoredValue::CLValue(cl_value);

        tracking_copy.emit_message(
            topic_key,
            topic_value,
            message_key,
            message_value,
            block_message_count_value,
            message,
        );
        Ok(())
    }
}

pub(crate) struct NewContractMessagesEmitter {
    /// This should be the Key::Hash with addr of the ContractPackage (if addressable entity is
    /// disabled) or Key::SmartContract otherwise
    key_of_package: Key,
    /// This should be the Key::Hash with addr of the Contract (if addressable entity is disabled)
    /// or Key::AddressableEntity(EntityAddr::SmartContract()) otherwise
    key_of_contract: Key,
    /// This should be the Key::Hash with addr of the ContractWasm (if addressable entity is
    /// disabled) or Key::Bytecode otherwise
    key_of_wasm: Key,
    /// Data which denotes what major version of the contract was installed
    version_major: u32,
    /// Data which denotes what minor version of the contract was installed
    version_minor: u32,
}

impl NewContractMessagesEmitter {
    pub(crate) fn new(
        key_of_package: Key,
        key_of_contract: Key,
        key_of_wasm: Key,
        version_major: u32,
        version_minor: u32,
    ) -> Self {
        Self {
            key_of_package,
            key_of_contract,
            key_of_wasm,
            version_major,
            version_minor,
        }
    }

    pub(crate) fn emit_contract_creation_messages<T>(
        &self,
        tracking_copy: &mut TrackingCopy<T>,
        current_blocktime: BlockTime,
    ) -> Result<(), MessageEmissionError>
    where
        T: StateReader<Key, StoredValue, Error = Error>,
    {
        let system_account_hash = PublicKey::System.to_account_hash().value();
        let entity_addr = EntityAddr::Account(system_account_hash);

        let topic_name = MESSAGING_PACKAGE_ADDR_TOPIC;
        let message_payload = MessagePayload::String(self.key_of_package.to_formatted_string());
        let emitter = MessageEmitter {};
        emitter.emit_message_for_entity(
            tracking_copy,
            entity_addr,
            topic_name,
            message_payload,
            current_blocktime,
        )?;

        let topic_name = MESSAGING_CONTRACT_ADDR_TOPIC;
        let message_payload = MessagePayload::String(self.key_of_contract.to_formatted_string());
        let emitter = MessageEmitter {};
        emitter.emit_message_for_entity(
            tracking_copy,
            entity_addr,
            topic_name,
            message_payload,
            current_blocktime,
        )?;

        let topic_name = MESSAGING_CONTRACT_BYTECODE_ADDR_TOPIC;
        let message_payload = MessagePayload::String(self.key_of_wasm.to_formatted_string());
        emitter.emit_message_for_entity(
            tracking_copy,
            entity_addr,
            topic_name,
            message_payload,
            current_blocktime,
        )?;

        let topic_name = MESSAGING_CONTRACT_VERSION_TOPIC;
        let message_payload =
            MessagePayload::String(format!("{}.{}", self.version_major, self.version_minor));
        emitter.emit_message_for_entity(
            tracking_copy,
            entity_addr,
            topic_name,
            message_payload,
            current_blocktime,
        )?;

        Ok(())
    }
}
