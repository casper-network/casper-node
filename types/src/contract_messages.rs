//! Data types for interacting with contract level messages.
use crate::{
    alloc::string::ToString,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    checksummed_hex, BlockTime, HashAddr, KEY_HASH_LENGTH,
};

use core::convert::TryFrom;

use alloc::{string::String, vec::Vec};
use core::{
    fmt::{self, Display, Formatter},
    num::ParseIntError,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The length in bytes of a [`MessageTopicHash`].
pub const MESSAGE_TOPIC_HASH_LENGTH: usize = 32;

/// The length of a message digest
pub const MESSAGE_DIGEST_LENGTH: usize = 32;

/// The hash of the name of the message topic.
pub type MessageTopicHash = [u8; MESSAGE_TOPIC_HASH_LENGTH];

const TOPIC_FORMATTED_STRING_PREFIX: &str = "topic-";

/// A newtype wrapping an array which contains the raw bytes of
/// the hash of the message emitted
#[derive(Default, PartialOrd, Ord, PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Message summary as a formatted string.")
)]
pub struct MessageSummary(
    #[cfg_attr(feature = "json-schema", schemars(skip, with = "String"))]
    pub  [u8; MESSAGE_DIGEST_LENGTH],
);

impl MessageSummary {
    /// Returns inner value of the message summary
    pub fn value(&self) -> [u8; MESSAGE_DIGEST_LENGTH] {
        self.0
    }
}

impl ToBytes for MessageSummary {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.append(&mut self.0.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for MessageSummary {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (summary, rem) = FromBytes::from_bytes(bytes)?;
        Ok((MessageSummary(summary), rem))
    }
}

/// Error while parsing a `[MessageTopicAddr]` from string.
#[derive(Debug)]
#[non_exhaustive]
pub enum FromStrError {
    /// No message index at the end of the string.
    MissingMessageIndex,
    /// Cannot parse entity hash.
    EntityHashParseError(String),
    /// Cannot parse message topic hash.
    MessageTopicParseError(String),
    /// Failed to decode address portion of URef.
    Hex(base16::DecodeError),
    /// Failed to parse an int.
    Int(ParseIntError),
}

impl From<base16::DecodeError> for FromStrError {
    fn from(error: base16::DecodeError) -> Self {
        FromStrError::Hex(error)
    }
}

impl From<ParseIntError> for FromStrError {
    fn from(error: ParseIntError) -> Self {
        FromStrError::Int(error)
    }
}

impl Display for FromStrError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            FromStrError::MissingMessageIndex => {
                write!(f, "no message index found at the end of the string")
            }
            FromStrError::EntityHashParseError(err) => {
                write!(f, "could not parse entity hash: {}", err)
            }
            FromStrError::MessageTopicParseError(err) => {
                write!(f, "could not parse topic hash: {}", err)
            }
            FromStrError::Hex(error) => {
                write!(f, "failed to decode address portion from hex: {}", error)
            }
            FromStrError::Int(error) => write!(f, "failed to parse an int: {}", error),
        }
    }
}

/// MessageTopicAddr
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct MessageAddr {
    /// The entity addr.
    entity_addr: HashAddr,
    /// The hash of the name of the message topic.
    topic_hash: MessageTopicHash,
    /// The message index.
    message_index: Option<u32>,
}

impl MessageAddr {
    /// Constructs a new topic address based on the addressable entity addr and the hash of the
    /// message topic name.
    pub const fn new_topic_addr(entity_addr: HashAddr, topic_hash: MessageTopicHash) -> Self {
        Self {
            entity_addr,
            topic_hash,
            message_index: None,
        }
    }

    /// Constructs a new message address based on the addressable entity addr, the hash of the
    /// message topic name and the message index in the topic.
    pub const fn new_message_addr(
        entity_addr: HashAddr,
        topic_hash: MessageTopicHash,
        message_index: u32,
    ) -> Self {
        Self {
            entity_addr,
            topic_hash,
            message_index: Some(message_index),
        }
    }

    /// Parses a formatted string into a [`MessageAddr`].
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        let (remainder, message_index) = match input.strip_prefix(TOPIC_FORMATTED_STRING_PREFIX) {
            Some(topic_string) => (topic_string, None),
            None => {
                let parts = input.splitn(2, '-').collect::<Vec<_>>();
                if parts.len() != 2 {
                    return Err(FromStrError::MissingMessageIndex);
                }
                (parts[0], Some(u32::from_str_radix(parts[1], 16)?))
            }
        };

        let bytes = checksummed_hex::decode(remainder)?;

        let entity_addr = <[u8; KEY_HASH_LENGTH]>::try_from(bytes[0..KEY_HASH_LENGTH].as_ref())
            .map_err(|err| FromStrError::EntityHashParseError(err.to_string()))?;

        let topic_hash =
            <[u8; MESSAGE_TOPIC_HASH_LENGTH]>::try_from(bytes[KEY_HASH_LENGTH..].as_ref())
                .map_err(|err| FromStrError::MessageTopicParseError(err.to_string()))?;

        Ok(MessageAddr {
            entity_addr,
            topic_hash,
            message_index,
        })
    }

    /// Returns the entity addr of this message topic.
    pub fn entity_addr(&self) -> HashAddr {
        self.entity_addr
    }
}

impl Display for MessageAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self.message_index {
            Some(index) => {
                write!(
                    f,
                    "{}{}-{:x}",
                    base16::encode_lower(&self.entity_addr),
                    base16::encode_lower(&self.topic_hash),
                    index,
                )
            }
            None => {
                write!(
                    f,
                    "{}{}{}",
                    TOPIC_FORMATTED_STRING_PREFIX,
                    base16::encode_lower(&self.entity_addr),
                    base16::encode_lower(&self.topic_hash),
                )
            }
        }
    }
}

impl ToBytes for MessageAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.append(&mut self.entity_addr.to_bytes()?);
        buffer.append(&mut self.topic_hash.to_bytes()?);
        buffer.append(&mut self.message_index.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.entity_addr.serialized_length()
            + self.topic_hash.serialized_length()
            + self.message_index.serialized_length()
    }
}

impl FromBytes for MessageAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (entity_addr, rem) = FromBytes::from_bytes(bytes)?;
        let (topic_hash, rem) = FromBytes::from_bytes(rem)?;
        let (message_index, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            MessageAddr {
                entity_addr,
                topic_hash,
                message_index,
            },
            rem,
        ))
    }
}

impl Distribution<MessageAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> MessageAddr {
        MessageAddr {
            entity_addr: rng.gen(),
            topic_hash: rng.gen(),
            message_index: rng.gen(),
        }
    }
}

/// Summary of a message topic that will be stored in global state.
#[derive(Eq, PartialEq, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct MessageTopicSummary {
    /// Number of messages in this topic.
    pub(crate) message_count: u32,
    /// Block timestamp in which these messages were emitted.
    pub(crate) blocktime: BlockTime,
}

impl MessageTopicSummary {
    /// Creates a new topic summary.
    pub fn new(message_count: u32, blocktime: BlockTime) -> Self {
        Self {
            message_count,
            blocktime,
        }
    }

    /// Returns the number of messages that were sent on this topic.
    pub fn message_count(&self) -> u32 {
        self.message_count
    }

    /// Returns the block time.
    pub fn blocktime(&self) -> BlockTime {
        self.blocktime
    }
}

impl ToBytes for MessageTopicSummary {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.append(&mut self.message_count.to_bytes()?);
        buffer.append(&mut self.blocktime.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.message_count.serialized_length() + self.blocktime.serialized_length()
    }
}

impl FromBytes for MessageTopicSummary {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (message_count, rem) = FromBytes::from_bytes(bytes)?;
        let (blocktime, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            MessageTopicSummary {
                message_count,
                blocktime,
            },
            rem,
        ))
    }
}

const MESSAGE_PAYLOAD_TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// Tag for an empty message payload.
pub const MESSAGE_PAYLOAD_EMPTY_TAG: u8 = 0;
/// Tag for a message payload that contains a human readable string.
pub const MESSAGE_PAYLOAD_STRING_TAG: u8 = 1;

/// The payload of the message emitted by an addressable entity during execution.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum MessagePayload {
    /// Empty message.
    Empty,
    /// Human readable string message.
    String(String),
}

impl MessagePayload {
    /// Creates a new [`MessagePayload`] from a [`String`].
    pub fn from_string(message: String) -> Self {
        Self::String(message)
    }
}

impl ToBytes for MessagePayload {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            MessagePayload::Empty => {
                buffer.insert(0, MESSAGE_PAYLOAD_EMPTY_TAG);
            }
            MessagePayload::String(message_string) => {
                buffer.insert(0, MESSAGE_PAYLOAD_STRING_TAG);
                buffer.extend(message_string.to_bytes()?);
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        MESSAGE_PAYLOAD_TAG_LENGTH
            + match self {
                MessagePayload::Empty => 0,
                MessagePayload::String(message_string) => message_string.serialized_length(),
            }
    }
}

impl FromBytes for MessagePayload {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            MESSAGE_PAYLOAD_EMPTY_TAG => Ok((Self::Empty, remainder)),
            MESSAGE_PAYLOAD_STRING_TAG => {
                let (message, remainder): (String, _) = FromBytes::from_bytes(remainder)?;
                Ok((Self::String(message), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// Message that was emitted by an addressable entity during execution.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Message {
    /// The identity of the entity that produced the message.
    #[cfg_attr(feature = "json-schema", schemars(with = "String"))]
    entity_addr: HashAddr,
    /// Message payload
    message: MessagePayload,
    /// Topic name
    topic: String,
    /// Message index in the topic
    index: u32,
}

impl Message {
    /// Creates new instance of [`Message`] with the specified source and message payload.
    pub fn new(source: HashAddr, message: MessagePayload, topic: String, index: u32) -> Self {
        Self {
            entity_addr: source,
            message,
            topic,
            index,
        }
    }
}

impl ToBytes for Message {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.append(&mut self.entity_addr.to_bytes()?);
        buffer.append(&mut self.message.to_bytes()?);
        buffer.append(&mut self.topic.to_bytes()?);
        buffer.append(&mut self.index.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.entity_addr.serialized_length()
            + self.message.serialized_length()
            + self.topic.serialized_length()
            + self.index.serialized_length()
    }
}

impl FromBytes for Message {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (entity_addr, rem) = FromBytes::from_bytes(bytes)?;
        let (message, rem) = FromBytes::from_bytes(rem)?;
        let (topic, rem) = FromBytes::from_bytes(rem)?;
        let (index, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            Message {
                entity_addr,
                message,
                topic,
                index,
            },
            rem,
        ))
    }
}

const TOPIC_OPERATION_ADD_TAG: u8 = 0;
const OPERATION_MAX_SERIALIZED_LEN: usize = 1;

/// Operations that can be performed on message topics.
pub enum MessageTopicOperation {
    /// Add a new message topic.
    Add,
}

impl MessageTopicOperation {
    /// Maximum serialized length of a message topic operation.
    pub const fn max_serialized_len() -> usize {
        OPERATION_MAX_SERIALIZED_LEN
    }
}

impl ToBytes for MessageTopicOperation {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            MessageTopicOperation::Add => buffer.push(TOPIC_OPERATION_ADD_TAG),
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        match self {
            MessageTopicOperation::Add => 1,
        }
    }
}

impl FromBytes for MessageTopicOperation {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            TOPIC_OPERATION_ADD_TAG => Ok((MessageTopicOperation::Add, remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{bytesrepr, KEY_HASH_LENGTH};

    use super::*;

    #[test]
    fn serialization_roundtrip() {
        let message_addr =
            MessageAddr::new_topic_addr([1; KEY_HASH_LENGTH], [2; MESSAGE_TOPIC_HASH_LENGTH]);
        bytesrepr::test_serialization_roundtrip(&message_addr);

        let topic_summary = MessageTopicSummary::new(100, BlockTime::new(1000));
        bytesrepr::test_serialization_roundtrip(&topic_summary);

        let string_message_payload =
            MessagePayload::from_string("new contract message".to_string());
        bytesrepr::test_serialization_roundtrip(&string_message_payload);

        let empty_message_payload = MessagePayload::Empty;
        bytesrepr::test_serialization_roundtrip(&empty_message_payload);

        let message = Message::new(
            [1; KEY_HASH_LENGTH],
            string_message_payload,
            "new contract message".to_string(),
            32,
        );
        bytesrepr::test_serialization_roundtrip(&message);
    }
}
