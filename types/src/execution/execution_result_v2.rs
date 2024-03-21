//! This file provides types to allow conversion from an EE `ExecutionResult` into a similar type
//! which can be serialized to a valid binary or JSON representation.
//!
//! It is stored as metadata related to a given transaction, and made available to clients via the
//! JSON-RPC API.

#[cfg(any(feature = "testing", test))]
use alloc::format;
use alloc::{string::String, vec::Vec};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Effects;
#[cfg(feature = "json-schema")]
use super::{TransformKindV2, TransformV2};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
#[cfg(feature = "json-schema")]
use crate::Key;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Gas, InitiatorAddr, Transfer, URef,
};

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
    effects.push(TransformV2::new(key1, TransformKindV2::AddUInt64(8u64)));
    effects.push(TransformV2::new(key2, TransformKindV2::Identity));

    let transfers = vec![Transfer::example().clone()];

    ExecutionResultV2 {
        effects,
        transfers,
        gas: Gas::new(123_456),
        payment: vec![PaymentInfo {
            source: URef::new([1; crate::UREF_ADDR_LENGTH], crate::AccessRights::READ),
        }],
        initiator: InitiatorAddr::from(crate::PublicKey::example().clone()),
        error_message: None,
    }
});

/// Breakdown of payments made to cover the cost.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct PaymentInfo {
    /// Source purse used for payment of the transaction.
    pub source: URef,
}

impl ToBytes for PaymentInfo {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.source.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.source.serialized_length()
    }
}

impl FromBytes for PaymentInfo {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (source, remainder) = URef::from_bytes(bytes)?;
        Ok((PaymentInfo { source }, remainder))
    }
}

/// The result of executing a single transaction.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ExecutionResultV2 {
    /// The effects of executing the transaction.
    pub effects: Effects,
    /// A record of transfers performed while executing the transaction.
    pub transfers: Vec<Transfer>,
    /// Identifier of the initiator of the transaction.
    pub initiator: InitiatorAddr,
    /// Breakdown of payments made to cover the cost.
    pub payment: Vec<PaymentInfo>,
    /// The gas consumed executing the transaction.
    pub gas: Gas,
    /// The error message associated with executing the transaction if the transaction failed.
    pub error_message: Option<String>,
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

        let transfer_count = rng.gen_range(0..6);
        let mut transfers = vec![];
        for _ in 0..transfer_count {
            transfers.push(Transfer::random(rng))
        }

        let gas = Gas::new(rng.gen::<u64>());

        let payment = vec![PaymentInfo { source: rng.gen() }];
        ExecutionResultV2 {
            effects,
            transfers,
            gas,
            payment,
            initiator: InitiatorAddr::random(rng),
            error_message: if rng.gen() {
                Some(format!("Error message {}", rng.gen::<u64>()))
            } else {
                None
            },
        }
    }
}

impl ToBytes for ExecutionResultV2 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.effects.write_bytes(writer)?;
        self.transfers.write_bytes(writer)?;
        self.initiator.write_bytes(writer)?;
        self.payment.write_bytes(writer)?;
        self.gas.write_bytes(writer)?;
        self.error_message.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.effects.serialized_length()
            + self.transfers.serialized_length()
            + self.initiator.serialized_length()
            + self.payment.serialized_length()
            + self.gas.serialized_length()
            + self.error_message.serialized_length()
    }
}

impl FromBytes for ExecutionResultV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (effects, remainder) = Effects::from_bytes(bytes)?;
        let (transfers, remainder) = Vec::<Transfer>::from_bytes(remainder)?;
        let (initiator, remainder) = InitiatorAddr::from_bytes(remainder)?;
        let (payment, remainder) = Vec::<PaymentInfo>::from_bytes(remainder)?;
        let (gas, remainder) = Gas::from_bytes(remainder)?;
        let (error_message, remainder) = Option::<String>::from_bytes(remainder)?;
        let execution_result = ExecutionResultV2 {
            effects,
            transfers,
            initiator,
            payment,
            gas,
            error_message,
        };
        Ok((execution_result, remainder))
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
