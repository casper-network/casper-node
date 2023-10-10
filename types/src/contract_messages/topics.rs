use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    checksummed_hex, BlockTime,
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
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use super::error::FromStrError;

/// The length in bytes of a topic name hash.
pub const TOPIC_NAME_HASH_LENGTH: usize = 32;
const MESSAGE_TOPIC_NAME_HASH: &str = "topic-name-";

/// The hash of the name of the message topic.
#[derive(Default, PartialOrd, Ord, PartialEq, Eq, Clone, Copy, Hash)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "The hash of the name of the message topic.")
)]
pub struct TopicNameHash(
    #[cfg_attr(feature = "json-schema", schemars(skip, with = "String"))]
    pub  [u8; TOPIC_NAME_HASH_LENGTH],
);

impl TopicNameHash {
    /// Returns a new [`TopicNameHash`] based on the specified value.
    pub const fn new(topic_name_hash: [u8; TOPIC_NAME_HASH_LENGTH]) -> TopicNameHash {
        TopicNameHash(topic_name_hash)
    }

    /// Returns inner value of the topic hash.
    pub fn value(&self) -> [u8; TOPIC_NAME_HASH_LENGTH] {
        self.0
    }

    /// Formats the [`TopicNameHash`] as a prefixed, hex-encoded string.
    pub fn to_formatted_string(self) -> String {
        format!(
            "{}{}",
            MESSAGE_TOPIC_NAME_HASH,
            base16::encode_lower(&self.0),
        )
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a [`TopicNameHash`].
    pub fn from_formatted_str(input: &str) -> Result<Self, FromStrError> {
        let remainder = input
            .strip_prefix(MESSAGE_TOPIC_NAME_HASH)
            .ok_or(FromStrError::InvalidPrefix)?;
        let bytes =
            <[u8; TOPIC_NAME_HASH_LENGTH]>::try_from(checksummed_hex::decode(remainder)?.as_ref())?;
        Ok(TopicNameHash(bytes))
    }
}

impl ToBytes for TopicNameHash {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.append(&mut self.0.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for TopicNameHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (hash, rem) = FromBytes::from_bytes(bytes)?;
        Ok((TopicNameHash(hash), rem))
    }
}

impl Serialize for TopicNameHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_formatted_string().serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for TopicNameHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let formatted_string = String::deserialize(deserializer)?;
            TopicNameHash::from_formatted_str(&formatted_string).map_err(SerdeError::custom)
        } else {
            let bytes = <[u8; TOPIC_NAME_HASH_LENGTH]>::deserialize(deserializer)?;
            Ok(TopicNameHash(bytes))
        }
    }
}

impl Display for TopicNameHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", base16::encode_lower(&self.0))
    }
}

impl Debug for TopicNameHash {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        write!(f, "MessageTopicHash({})", base16::encode_lower(&self.0))
    }
}

impl Distribution<TopicNameHash> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> TopicNameHash {
        TopicNameHash(rng.gen())
    }
}

impl From<[u8; TOPIC_NAME_HASH_LENGTH]> for TopicNameHash {
    fn from(value: [u8; TOPIC_NAME_HASH_LENGTH]) -> Self {
        TopicNameHash(value)
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

const TOPIC_OPERATION_ADD_TAG: u8 = 0;
const OPERATION_MAX_SERIALIZED_LEN: usize = 1;

/// Operations that can be performed on message topics.
#[derive(Debug, PartialEq)]
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
    use crate::bytesrepr;

    use super::*;

    #[test]
    fn serialization_roundtrip() {
        let topic_name_hash = TopicNameHash::new([0x4du8; TOPIC_NAME_HASH_LENGTH]);
        bytesrepr::test_serialization_roundtrip(&topic_name_hash);

        let topic_summary = MessageTopicSummary::new(10, BlockTime::new(100));
        bytesrepr::test_serialization_roundtrip(&topic_summary);

        let topic_operation = MessageTopicOperation::Add;
        bytesrepr::test_serialization_roundtrip(&topic_operation);
    }
}
