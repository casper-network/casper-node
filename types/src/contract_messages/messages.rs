use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    AddressableEntityHash,
};

use alloc::{string::String, vec::Vec};
use core::fmt::Debug;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Alphanumeric, DistString, Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::TopicNameHash;

/// Collection of multiple messages.
pub type Messages = Vec<Message>;

/// The length of a message digest
pub const MESSAGE_CHECKSUM_LENGTH: usize = 32;

/// A newtype wrapping an array which contains the raw bytes of
/// the hash of the message emitted.
#[derive(Default, PartialOrd, Ord, PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Message checksum as a formatted string.")
)]
pub struct MessageChecksum(
    #[cfg_attr(feature = "json-schema", schemars(skip, with = "String"))]
    pub  [u8; MESSAGE_CHECKSUM_LENGTH],
);

impl MessageChecksum {
    /// Returns inner value of the message checksum.
    pub fn value(&self) -> [u8; MESSAGE_CHECKSUM_LENGTH] {
        self.0
    }
}

impl ToBytes for MessageChecksum {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.append(&mut self.0.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for MessageChecksum {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (checksum, rem) = FromBytes::from_bytes(bytes)?;
        Ok((MessageChecksum(checksum), rem))
    }
}

const MESSAGE_PAYLOAD_TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// Tag for a message payload that contains a human readable string.
pub const MESSAGE_PAYLOAD_STRING_TAG: u8 = 0;

/// The payload of the message emitted by an addressable entity during execution.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum MessagePayload {
    /// Human readable string message.
    String(String),
}

impl<T> From<T> for MessagePayload
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        Self::String(value.into())
    }
}

impl ToBytes for MessagePayload {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
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
                MessagePayload::String(message_string) => message_string.serialized_length(),
            }
    }
}

impl FromBytes for MessagePayload {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
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
    entity_addr: AddressableEntityHash,
    /// The payload of the message.
    message: MessagePayload,
    /// The name of the topic on which the message was emitted on.
    topic_name: String,
    /// The hash of the name of the topic.
    topic_name_hash: TopicNameHash,
    /// Message index in the topic.
    index: u32,
}

impl Message {
    /// Creates new instance of [`Message`] with the specified source and message payload.
    pub fn new(
        source: AddressableEntityHash,
        message: MessagePayload,
        topic_name: String,
        topic_name_hash: TopicNameHash,
        index: u32,
    ) -> Self {
        Self {
            entity_addr: source,
            message,
            topic_name,
            topic_name_hash,
            index,
        }
    }

    /// Returns a reference to the identity of the entity that produced the message.
    pub fn entity_addr(&self) -> &AddressableEntityHash {
        &self.entity_addr
    }

    /// Returns a reference to the payload of the message.
    pub fn payload(&self) -> &MessagePayload {
        &self.message
    }

    /// Returns a reference to the name of the topic on which the message was emitted on.
    pub fn topic_name(&self) -> &String {
        &self.topic_name
    }

    /// Returns a reference to the hash of the name of the topic.
    pub fn topic_name_hash(&self) -> &TopicNameHash {
        &self.topic_name_hash
    }

    /// Returns the index of the message in the topic.
    pub fn index(&self) -> u32 {
        self.index
    }
}

impl ToBytes for Message {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.append(&mut self.entity_addr.to_bytes()?);
        buffer.append(&mut self.message.to_bytes()?);
        buffer.append(&mut self.topic_name.to_bytes()?);
        buffer.append(&mut self.topic_name_hash.to_bytes()?);
        buffer.append(&mut self.index.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.entity_addr.serialized_length()
            + self.message.serialized_length()
            + self.topic_name.serialized_length()
            + self.topic_name_hash.serialized_length()
            + self.index.serialized_length()
    }
}

impl FromBytes for Message {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (entity_addr, rem) = FromBytes::from_bytes(bytes)?;
        let (message, rem) = FromBytes::from_bytes(rem)?;
        let (topic_name, rem) = FromBytes::from_bytes(rem)?;
        let (topic_name_hash, rem) = FromBytes::from_bytes(rem)?;
        let (index, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            Message {
                entity_addr,
                message,
                topic_name,
                topic_name_hash,
                index,
            },
            rem,
        ))
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<Message> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Message {
        let topic_name = Alphanumeric.sample_string(rng, 32);
        let topic_name_hash = crate::crypto::blake2b(&topic_name).into();
        let message = Alphanumeric.sample_string(rng, 64).into();

        Message {
            entity_addr: rng.gen(),
            message,
            topic_name,
            topic_name_hash,
            index: rng.gen(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{bytesrepr, contract_messages::topics::TOPIC_NAME_HASH_LENGTH, KEY_HASH_LENGTH};

    use super::*;

    #[test]
    fn serialization_roundtrip() {
        let message_checksum = MessageChecksum([1; MESSAGE_CHECKSUM_LENGTH]);
        bytesrepr::test_serialization_roundtrip(&message_checksum);

        let message_payload = "message payload".into();
        bytesrepr::test_serialization_roundtrip(&message_payload);

        let message = Message::new(
            [1; KEY_HASH_LENGTH].into(),
            message_payload,
            "test_topic".to_string(),
            TopicNameHash::new([0x4du8; TOPIC_NAME_HASH_LENGTH]),
            10,
        );
        bytesrepr::test_serialization_roundtrip(&message);
    }
}
