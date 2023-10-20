//! Data types for interacting with contract level messages.

mod error;
mod messages;
mod topics;

pub use error::FromStrError;
pub use messages::{Message, MessageChecksum, MessagePayload, Messages};
pub use topics::{
    MessageTopicOperation, MessageTopicSummary, TopicNameHash, TOPIC_NAME_HASH_LENGTH,
};

use crate::{
    alloc::string::ToString,
    bytesrepr::{self, FromBytes, ToBytes},
    checksummed_hex, AddressableEntityHash, KEY_HASH_LENGTH,
};

use core::convert::TryFrom;

use alloc::{string::String, vec::Vec};
use core::fmt::{Debug, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const TOPIC_FORMATTED_STRING_PREFIX: &str = "topic-";
const MESSAGE_ADDR_PREFIX: &str = "message-";

/// MessageTopicAddr
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct MessageAddr {
    /// The entity addr.
    entity_addr: AddressableEntityHash,
    /// The hash of the name of the message topic.
    topic_name_hash: TopicNameHash,
    /// The message index.
    message_index: Option<u32>,
}

impl MessageAddr {
    /// Constructs a new topic address based on the addressable entity addr and the hash of the
    /// message topic name.
    pub const fn new_topic_addr(
        entity_addr: AddressableEntityHash,
        topic_name_hash: TopicNameHash,
    ) -> Self {
        Self {
            entity_addr,
            topic_name_hash,
            message_index: None,
        }
    }

    /// Constructs a new message address based on the addressable entity addr, the hash of the
    /// message topic name and the message index in the topic.
    pub const fn new_message_addr(
        entity_addr: AddressableEntityHash,
        topic_name_hash: TopicNameHash,
        message_index: u32,
    ) -> Self {
        Self {
            entity_addr,
            topic_name_hash,
            message_index: Some(message_index),
        }
    }

    /// Formats the [`MessageAddr`] as a prefixed, hex-encoded string.
    pub fn to_formatted_string(self) -> String {
        match self.message_index {
            Some(index) => {
                format!(
                    "{}{}-{}-{:x}",
                    MESSAGE_ADDR_PREFIX,
                    base16::encode_lower(&self.entity_addr),
                    self.topic_name_hash.to_formatted_string(),
                    index,
                )
            }
            None => {
                format!(
                    "{}{}{}-{}",
                    MESSAGE_ADDR_PREFIX,
                    TOPIC_FORMATTED_STRING_PREFIX,
                    base16::encode_lower(&self.entity_addr),
                    self.topic_name_hash.to_formatted_string(),
                )
            }
        }
    }

    /// Parses a formatted string into a [`MessageAddr`].
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(MESSAGE_ADDR_PREFIX)
            .ok_or(FromStrError::InvalidPrefix)?;

        let (remainder, message_index) = match remainder.strip_prefix(TOPIC_FORMATTED_STRING_PREFIX)
        {
            Some(topic_string) => (topic_string, None),
            None => {
                let (remainder, message_index_str) = remainder
                    .rsplit_once('-')
                    .ok_or(FromStrError::MissingMessageIndex)?;
                (remainder, Some(u32::from_str_radix(message_index_str, 16)?))
            }
        };

        let (entity_addr_str, topic_name_hash_str) = remainder
            .split_once('-')
            .ok_or(FromStrError::MissingMessageIndex)?;

        let bytes = checksummed_hex::decode(entity_addr_str)?;
        let entity_addr = <AddressableEntityHash>::try_from(bytes[0..KEY_HASH_LENGTH].as_ref())
            .map_err(|err| FromStrError::EntityHashParseError(err.to_string()))?;

        let topic_name_hash = TopicNameHash::from_formatted_str(topic_name_hash_str)?;
        Ok(MessageAddr {
            entity_addr,
            topic_name_hash,
            message_index,
        })
    }

    /// Returns the entity addr of this message topic.
    pub fn entity_addr(&self) -> AddressableEntityHash {
        self.entity_addr
    }
}

impl Display for MessageAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self.message_index {
            Some(index) => {
                write!(
                    f,
                    "{}-{}-{:x}",
                    base16::encode_lower(&self.entity_addr),
                    self.topic_name_hash,
                    index,
                )
            }
            None => {
                write!(
                    f,
                    "{}-{}",
                    base16::encode_lower(&self.entity_addr),
                    self.topic_name_hash,
                )
            }
        }
    }
}

impl ToBytes for MessageAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.append(&mut self.entity_addr.to_bytes()?);
        buffer.append(&mut self.topic_name_hash.to_bytes()?);
        buffer.append(&mut self.message_index.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.entity_addr.serialized_length()
            + self.topic_name_hash.serialized_length()
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
                topic_name_hash: topic_hash,
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
            topic_name_hash: rng.gen(),
            message_index: rng.gen(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{bytesrepr, KEY_HASH_LENGTH};

    use super::{topics::TOPIC_NAME_HASH_LENGTH, *};

    #[test]
    fn serialization_roundtrip() {
        let topic_addr = MessageAddr::new_topic_addr(
            [1; KEY_HASH_LENGTH].into(),
            [2; TOPIC_NAME_HASH_LENGTH].into(),
        );
        bytesrepr::test_serialization_roundtrip(&topic_addr);

        let message_addr = MessageAddr::new_message_addr(
            [1; KEY_HASH_LENGTH].into(),
            [2; TOPIC_NAME_HASH_LENGTH].into(),
            3,
        );
        bytesrepr::test_serialization_roundtrip(&message_addr);
    }
}
