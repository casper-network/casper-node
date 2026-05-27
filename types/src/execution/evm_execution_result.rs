//! EVM transaction execution result types.

use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Effects;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    evm, Gas, U512,
};

/// The result of executing a single EVM transaction.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EvmExecutionResult {
    /// Who initiated this EVM transaction.
    pub initiator: evm::Address,
    /// The current Casper gas price used for fee accounting.
    pub current_price: u8,
    /// The maximum allowed gas limit for this transaction.
    pub limit: Gas,
    /// How much was paid for this transaction.
    pub cost: U512,
    /// How much unconsumed gas was refunded, if any.
    pub refund: U512,
    /// The size estimate of the transaction.
    pub size_estimate: u64,
    /// The effects of executing this transaction.
    pub effects: Effects,
    /// EVM-native receipt data used by Ethereum JSON-RPC projections.
    pub receipt: evm::Receipt,
}

impl EvmExecutionResult {
    /// Returns a random `EvmExecutionResult`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let limit = Gas::new(rng.gen::<u64>());
        let gas_price = rng.gen_range(1..6);
        let cost = limit.value() * U512::from(gas_price);
        EvmExecutionResult {
            initiator: evm::Address::new(rng.gen()),
            current_price: gas_price,
            limit,
            cost,
            refund: rng.gen::<u64>().into(),
            size_estimate: rng.gen(),
            effects: Effects::random(rng),
            receipt: evm::Receipt::random(rng),
        }
    }
}

impl ToBytes for EvmExecutionResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.initiator.serialized_length()
            + self.current_price.serialized_length()
            + self.limit.serialized_length()
            + self.cost.serialized_length()
            + self.refund.serialized_length()
            + self.size_estimate.serialized_length()
            + self.effects.serialized_length()
            + self.receipt.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.initiator.write_bytes(writer)?;
        self.current_price.write_bytes(writer)?;
        self.limit.write_bytes(writer)?;
        self.cost.write_bytes(writer)?;
        self.refund.write_bytes(writer)?;
        self.size_estimate.write_bytes(writer)?;
        self.effects.write_bytes(writer)?;
        self.receipt.write_bytes(writer)
    }
}

impl FromBytes for EvmExecutionResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (initiator, remainder) = evm::Address::from_bytes(bytes)?;
        let (current_price, remainder) = u8::from_bytes(remainder)?;
        let (limit, remainder) = Gas::from_bytes(remainder)?;
        let (cost, remainder) = U512::from_bytes(remainder)?;
        let (refund, remainder) = U512::from_bytes(remainder)?;
        let (size_estimate, remainder) = u64::from_bytes(remainder)?;
        let (effects, remainder) = Effects::from_bytes(remainder)?;
        let (receipt, remainder) = evm::Receipt::from_bytes(remainder)?;
        Ok((
            EvmExecutionResult {
                initiator,
                current_price,
                limit,
                cost,
                refund,
                size_estimate,
                effects,
                receipt,
            },
            remainder,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            let execution_result = EvmExecutionResult::random(rng);
            bytesrepr::test_serialization_roundtrip(&execution_result);
        }
    }

    #[test]
    fn json_schema() {
        #[cfg(feature = "json-schema")]
        {
            let schema = schemars::schema_for!(EvmExecutionResult);
            serde_json::to_value(&schema).unwrap();
        }
    }

    #[test]
    fn receipt_bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            let receipt = evm::Receipt::random(rng);
            bytesrepr::test_serialization_roundtrip(&receipt);
            for log in &receipt.logs {
                bytesrepr::test_serialization_roundtrip(log);
            }
        }
    }

    #[test]
    fn receipt_json_schema() {
        #[cfg(feature = "json-schema")]
        {
            let receipt_schema = schemars::schema_for!(evm::Receipt);
            let log_schema = schemars::schema_for!(evm::Log);
            serde_json::to_value(&receipt_schema).unwrap();
            serde_json::to_value(&log_schema).unwrap();
        }
    }
}
