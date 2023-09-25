//! Data types for interacting with contract level messages.

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    HashAddr,
};
use alloc::vec::Vec;
use core::fmt::{Display, Formatter};

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

/// The hash of the name of the message topic.
pub type MessageTopicHash = [u8; MESSAGE_TOPIC_HASH_LENGTH];

/// MessageAddr
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct MessageTopicAddr {
    /// The entity addr.
    entity_addr: HashAddr,
    /// The hash of the name of the message topic.
    topic_hash: MessageTopicHash,
}

impl MessageTopicAddr {
    /// Constructs a new [`MessageAddr`] based on the addressable entity addr and the hash of the
    /// message topic name.
    pub const fn new(entity_addr: HashAddr, topic_hash: MessageTopicHash) -> Self {
        Self {
            entity_addr,
            topic_hash,
        }
    }
}

impl Display for MessageTopicAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}{}",
            base16::encode_lower(&self.entity_addr),
            base16::encode_lower(&self.topic_hash)
        )
    }
}

impl ToBytes for MessageTopicAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.append(&mut self.entity_addr.to_bytes()?);
        buffer.append(&mut self.topic_hash.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.entity_addr.serialized_length() + self.topic_hash.serialized_length()
    }
}

impl FromBytes for MessageTopicAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (entity_addr, rem) = FromBytes::from_bytes(bytes)?;
        let (topic_hash, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            MessageTopicAddr {
                entity_addr,
                topic_hash,
            },
            rem,
        ))
    }
}

impl Distribution<MessageTopicAddr> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> MessageTopicAddr {
        MessageTopicAddr {
            entity_addr: rng.gen(),
            topic_hash: rng.gen(),
        }
    }
}
