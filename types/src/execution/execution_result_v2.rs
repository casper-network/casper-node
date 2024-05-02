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
    Gas, InitiatorAddr, Transfer, URef, U512,
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
        initiator: InitiatorAddr::from(crate::PublicKey::example().clone()),
        error_message: None,
        limit: Gas::new(123_456),
        consumed: Gas::new(100_000),
        cost: U512::from(246_912),
        payment: vec![PaymentInfo {
            source: URef::new([1; crate::UREF_ADDR_LENGTH], crate::AccessRights::READ),
        }],
        size_estimate: Transfer::example().serialized_length() as u64,
        transfers,
        effects,
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
    /// Who initiated this transaction.
    pub initiator: InitiatorAddr,
    /// If there is no error message, this execution was processed successfully.
    /// If there is an error message, this execution failed to fully process for the stated reason.
    pub error_message: Option<String>,
    /// What was the maximum allowed gas limit for this transaction?.
    pub limit: Gas,
    /// How much gas was consumed executing this transaction.
    pub consumed: Gas,
    /// How much was paid for this transaction.
    pub cost: U512,
    /// Breakdown of payments made to cover the cost.
    pub payment: Vec<PaymentInfo>,
    /// A record of transfers performed while executing this transaction.
    pub transfers: Vec<Transfer>,
    /// The size estimate of the transaction
    pub size_estimate: u64,
    /// The effects of executing this transaction.
    pub effects: Effects,
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

        let limit = Gas::new(rng.gen::<u64>());
        let gas_price = rng.gen_range(1..6);
        // cost = the limit * the price
        let cost = limit.value() * U512::from(gas_price);
        let range = limit.value().as_u64();

        // can range from 0 to limit
        let consumed = limit - Gas::new(rng.gen_range(0..=range));
        let size_estimate = rng.gen();

        let payment = vec![PaymentInfo { source: rng.gen() }];
        ExecutionResultV2 {
            initiator: InitiatorAddr::random(rng),
            effects,
            transfers,
            cost,
            payment,
            limit,
            consumed,
            size_estimate,
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
        self.initiator.write_bytes(writer)?; // initiator should logically be first
        self.error_message.write_bytes(writer)?;
        self.limit.write_bytes(writer)?;
        self.consumed.write_bytes(writer)?;
        self.cost.write_bytes(writer)?;
        self.payment.write_bytes(writer)?;
        self.transfers.write_bytes(writer)?;
        self.size_estimate.write_bytes(writer)?;
        self.effects.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.initiator.serialized_length()
            + self.error_message.serialized_length()
            + self.limit.serialized_length()
            + self.consumed.serialized_length()
            + self.cost.serialized_length()
            + self.payment.serialized_length()
            + self.transfers.serialized_length()
            + self.size_estimate.serialized_length()
            + self.effects.serialized_length()
    }
}

impl FromBytes for ExecutionResultV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (initiator, remainder) = InitiatorAddr::from_bytes(bytes)?;
        let (error_message, remainder) = Option::<String>::from_bytes(remainder)?;
        let (limit, remainder) = Gas::from_bytes(remainder)?;
        let (consumed, remainder) = Gas::from_bytes(remainder)?;
        let (cost, remainder) = U512::from_bytes(remainder)?;
        let (payment, remainder) = Vec::<PaymentInfo>::from_bytes(remainder)?;
        let (transfers, remainder) = Vec::<Transfer>::from_bytes(remainder)?;
        let (size_estimate, remainder) = FromBytes::from_bytes(remainder)?;
        let (effects, remainder) = Effects::from_bytes(remainder)?;
        let execution_result = ExecutionResultV2 {
            initiator,
            error_message,
            limit,
            consumed,
            cost,
            payment,
            transfers,
            size_estimate,
            effects,
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
