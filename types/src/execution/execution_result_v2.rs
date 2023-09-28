//! This file provides types to allow conversion from an EE `ExecutionResult` into a similar type
//! which can be serialized to a valid binary or JSON representation.
//!
//! It is stored as metadata related to a given deploy, and made available to clients via the
//! JSON-RPC API.

#[cfg(any(feature = "testing", test))]
use alloc::format;
use alloc::{string::String, vec::Vec};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use itertools::Itertools;
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Alphanumeric, DistString, Standard},
    prelude::Distribution,
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Effects;
#[cfg(feature = "json-schema")]
use super::{Transform, TransformKind};
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, RESULT_ERR_TAG, RESULT_OK_TAG, U8_SERIALIZED_LENGTH},
    contract_messages::Message,
    TransferAddr, U512,
};
#[cfg(any(feature = "testing", test))]
use crate::{contract_messages::MessagePayload, testing::TestRng};
#[cfg(feature = "json-schema")]
use crate::{Key, KEY_HASH_LENGTH};

#[cfg(feature = "json-schema")]
static EXECUTION_RESULT: Lazy<ExecutionResultV2> = Lazy::new(|| {
    let key1 = Key::from_formatted_str(
        "account-hash-2c4a11c062a8a337bfc97e27fd66291caeb2c65865dcb5d3ef3759c4c97efecb",
    )
    .unwrap();
    let key2 = Key::from_formatted_str(
        "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1",
    )
    .unwrap();
    let mut effects = Effects::new();
    effects.push(Transform::new(key1, TransformKind::AddUInt64(8u64)));
    effects.push(Transform::new(key2, TransformKind::Identity));

    let transfers = vec![
        TransferAddr::new([89; KEY_HASH_LENGTH]),
        TransferAddr::new([130; KEY_HASH_LENGTH]),
    ];

    ExecutionResultV2::Success {
        effects,
        transfers,
        cost: U512::from(123_456),
        messages: Vec::default(),
    }
});

/// The result of executing a single deploy.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum ExecutionResultV2 {
    /// The result of a failed execution.
    Failure {
        /// The effects of executing the deploy.
        effects: Effects,
        /// A record of transfers performed while executing the deploy.
        transfers: Vec<TransferAddr>,
        /// The cost in Motes of executing the deploy.
        cost: U512,
        /// The error message associated with executing the deploy.
        error_message: String,
        /// Messages that were emitted during execution.
        messages: Vec<Message>,
    },
    /// The result of a successful execution.
    Success {
        /// The effects of executing the deploy.
        effects: Effects,
        /// A record of transfers performed while executing the deploy.
        transfers: Vec<TransferAddr>,
        /// The cost in Motes of executing the deploy.
        cost: U512,
        /// Messages that were emitted during execution.
        messages: Vec<Message>,
    },
}

#[cfg(any(feature = "testing", test))]
impl Distribution<ExecutionResultV2> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ExecutionResultV2 {
        let transfer_count = rng.gen_range(0..6);
        let mut transfers = Vec::new();
        for _ in 0..transfer_count {
            transfers.push(TransferAddr::new(rng.gen()))
        }

        let effects = Effects::random(rng);
        let messages = effects
            .transforms()
            .iter()
            .filter_map(|transform| {
                if let Key::Message(addr) = transform.key() {
                    Some(Message::new(
                        addr.entity_addr(),
                        MessagePayload::from_string(format!("random_msg: {}", rng.gen::<u64>())),
                        Alphanumeric.sample_string(rng, 32),
                        rng.gen::<u32>(),
                    ))
                } else {
                    None
                }
            })
            .collect_vec();

        if rng.gen() {
            ExecutionResultV2::Failure {
                effects,
                transfers,
                cost: rng.gen::<u64>().into(),
                error_message: format!("Error message {}", rng.gen::<u64>()),
                messages,
            }
        } else {
            ExecutionResultV2::Success {
                effects,
                transfers,
                cost: rng.gen::<u64>().into(),
                messages,
            }
        }
    }
}

impl ExecutionResultV2 {
    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &EXECUTION_RESULT
    }

    /// Returns a random `ExecutionResultV2`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let effects = Effects::random(rng);
        let messages = effects
            .transforms()
            .iter()
            .filter_map(|transform| {
                if let Key::Message(addr) = transform.key() {
                    Some(Message::new(
                        addr.entity_addr(),
                        MessagePayload::from_string(format!("random_msg: {}", rng.gen::<u64>())),
                        Alphanumeric.sample_string(rng, 32),
                        rng.gen::<u32>(),
                    ))
                } else {
                    None
                }
            })
            .collect_vec();

        let transfer_count = rng.gen_range(0..6);
        let mut transfers = vec![];
        for _ in 0..transfer_count {
            transfers.push(TransferAddr::new(rng.gen()))
        }

        let cost = U512::from(rng.gen::<u64>());

        if rng.gen() {
            ExecutionResultV2::Failure {
                effects,
                transfers,
                cost,
                error_message: format!("Error message {}", rng.gen::<u64>()),
                messages,
            }
        } else {
            ExecutionResultV2::Success {
                effects,
                transfers,
                cost,
                messages,
            }
        }
    }
}

impl ToBytes for ExecutionResultV2 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            ExecutionResultV2::Failure {
                effects,
                transfers,
                cost,
                error_message,
                messages,
            } => {
                RESULT_ERR_TAG.write_bytes(writer)?;
                effects.write_bytes(writer)?;
                transfers.write_bytes(writer)?;
                cost.write_bytes(writer)?;
                error_message.write_bytes(writer)?;
                messages.write_bytes(writer)
            }
            ExecutionResultV2::Success {
                effects,
                transfers,
                cost,
                messages,
            } => {
                RESULT_OK_TAG.write_bytes(writer)?;
                effects.write_bytes(writer)?;
                transfers.write_bytes(writer)?;
                cost.write_bytes(writer)?;
                messages.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                ExecutionResultV2::Failure {
                    effects,
                    transfers,
                    cost,
                    error_message,
                    messages,
                } => {
                    effects.serialized_length()
                        + transfers.serialized_length()
                        + cost.serialized_length()
                        + error_message.serialized_length()
                        + messages.serialized_length()
                }
                ExecutionResultV2::Success {
                    effects,
                    transfers,
                    cost,
                    messages,
                } => {
                    effects.serialized_length()
                        + transfers.serialized_length()
                        + cost.serialized_length()
                        + messages.serialized_length()
                }
            }
    }
}

impl FromBytes for ExecutionResultV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            RESULT_ERR_TAG => {
                let (effects, remainder) = Effects::from_bytes(remainder)?;
                let (transfers, remainder) = Vec::<TransferAddr>::from_bytes(remainder)?;
                let (cost, remainder) = U512::from_bytes(remainder)?;
                let (error_message, remainder) = String::from_bytes(remainder)?;
                let (messages, remainder) = Vec::<Message>::from_bytes(remainder)?;
                let execution_result = ExecutionResultV2::Failure {
                    effects,
                    transfers,
                    cost,
                    error_message,
                    messages,
                };
                Ok((execution_result, remainder))
            }
            RESULT_OK_TAG => {
                let (effects, remainder) = Effects::from_bytes(remainder)?;
                let (transfers, remainder) = Vec::<TransferAddr>::from_bytes(remainder)?;
                let (cost, remainder) = U512::from_bytes(remainder)?;
                let (messages, remainder) = Vec::<Message>::from_bytes(remainder)?;
                let execution_result = ExecutionResultV2::Success {
                    effects,
                    transfers,
                    cost,
                    messages,
                };
                Ok((execution_result, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            let execution_result = ExecutionResultV2::random(rng);
            bytesrepr::test_serialization_roundtrip(&execution_result);
        }
    }
}
