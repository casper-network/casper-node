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
    /// Maximum size (in bytes) of a topic name.
    max_topic_name_size: u32,
    /// Maximum message size in bytes.
    max_message_size: u32,
}

impl MessagesLimits {
    /// Check if a specified topic `name_size` exceeds the configured value.
    pub fn topic_name_size_within_limits(&self, name_size: u32) -> Result<(), Error> {
        if name_size > self.max_topic_name_size {
            Err(Error::TopicNameSizeExceeded(
                self.max_topic_name_size,
                name_size,
            ))
        } else {
            Ok(())
        }
    }

    /// Check if a specified message size exceeds the configured max value.
    pub fn message_size_within_limits(&self, message_size: u32) -> Result<(), Error> {
        if message_size > self.max_message_size {
            Err(Error::MessageTooLarge(self.max_message_size, message_size))
        } else {
            Ok(())
        }
    }
}

impl Default for MessagesLimits {
    fn default() -> Self {
        Self {
            max_topic_name_size: 256,
            max_message_size: 1024,
        }
    }
}

impl ToBytes for MessagesLimits {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.max_topic_name_size.to_bytes()?);
        ret.append(&mut self.max_message_size.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.max_topic_name_size.serialized_length() + self.max_message_size.serialized_length()
    }
}

impl FromBytes for MessagesLimits {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (max_topic_name_size, rem) = FromBytes::from_bytes(bytes)?;
        let (max_message_size, rem) = FromBytes::from_bytes(rem)?;

        Ok((
            MessagesLimits {
                max_topic_name_size,
                max_message_size,
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
        ) -> MessagesLimits {
            MessagesLimits {
                max_topic_name_size,
                max_message_size,
            }
        }
    }
}
