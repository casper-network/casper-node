#[cfg(feature = "datasize")]
use datasize::DataSize;
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::bytesrepr::{self, FromBytes, ToBytes};

/// Configuration for messages limits.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct MessagesLimits {
    /// Maximum size (in bytes) of a topic name string.
    pub max_topic_name_size: u32,
    /// Maximum message size in bytes.
    pub max_message_size: u32,
    /// Maximum number of topics that a contract can register.
    pub max_topics_per_contract: u32,
}

impl MessagesLimits {
    /// Check if a specified message size exceeds the configured max value.
    pub fn message_size_within_limits(&self, message_size: u32) -> Result<(), Error> {
        if message_size > self.max_message_size {
            Err(Error::MessageTooLarge(self.max_message_size, message_size))
        } else {
            Ok(())
        }
    }

    /// Returns the max number of topics a contract can register.
    pub fn max_topics_per_contract(&self) -> u32 {
        self.max_topics_per_contract
    }

    /// Returns the maximum allowed size for the topic name string.
    pub fn max_topic_name_size(&self) -> u32 {
        self.max_topic_name_size
    }
}

impl Default for MessagesLimits {
    fn default() -> Self {
        Self {
            max_topic_name_size: 256,
            max_message_size: 1024,
            max_topics_per_contract: 128,
        }
    }
}

impl ToBytes for MessagesLimits {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.max_topic_name_size.to_bytes()?);
        ret.append(&mut self.max_message_size.to_bytes()?);
        ret.append(&mut self.max_topics_per_contract.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.max_topic_name_size.serialized_length()
            + self.max_message_size.serialized_length()
            + self.max_topics_per_contract.serialized_length()
    }
}

impl FromBytes for MessagesLimits {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (max_topic_name_size, rem) = FromBytes::from_bytes(bytes)?;
        let (max_message_size, rem) = FromBytes::from_bytes(rem)?;
        let (max_topics_per_contract, rem) = FromBytes::from_bytes(rem)?;

        Ok((
            MessagesLimits {
                max_topic_name_size,
                max_message_size,
                max_topics_per_contract,
            },
            rem,
        ))
    }
}

impl Distribution<MessagesLimits> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> MessagesLimits {
        MessagesLimits {
            max_topic_name_size: rng.gen(),
            max_message_size: rng.gen(),
            max_topics_per_contract: rng.gen(),
        }
    }
}

/// Possible execution errors.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum Error {
    /// Topic name size exceeded.
    #[error(
        "Topic name size is too large: expected less then {} bytes, got {} bytes",
        _0,
        _1
    )]
    TopicNameSizeExceeded(u32, u32),
    /// Message size exceeded.
    #[error("Message size cannot exceed {} bytes; actual size {}", _0, _1)]
    MessageTooLarge(u32, u32),
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use super::MessagesLimits;

    prop_compose! {
        pub fn message_limits_arb()(
            max_topic_name_size in num::u32::ANY,
            max_message_size in num::u32::ANY,
            max_topics_per_contract in num::u32::ANY,
        ) -> MessagesLimits {
            MessagesLimits {
                max_topic_name_size,
                max_message_size,
                max_topics_per_contract,
            }
        }
    }
}
