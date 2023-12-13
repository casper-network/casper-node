//! The result of the speculative execution request.

use alloc::vec::Vec;

#[cfg(test)]
use core::iter;

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::testing::TestRng;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    contract_messages::Messages,
    execution::ExecutionResultV2,
};

/// Result of the speculative execution request.
#[derive(Debug, PartialEq)]
pub struct SpeculativeExecutionResult {
    /// Result of the execution.
    pub execution_result: ExecutionResultV2,
    /// Messages emitted during execution.
    pub messages: Messages,
}

impl SpeculativeExecutionResult {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        let messages_count = rng.gen_range(0..10);
        let messages = iter::repeat(())
            .map(|_| rng.gen())
            .take(messages_count)
            .collect();

        let execution_result = ExecutionResultV2::random(rng);

        Self {
            execution_result,
            messages,
        }
    }
}

impl ToBytes for SpeculativeExecutionResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let SpeculativeExecutionResult {
            execution_result,
            messages,
        } = self;
        execution_result.write_bytes(writer)?;
        messages.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.execution_result.serialized_length() + self.messages.serialized_length()
    }
}

impl FromBytes for SpeculativeExecutionResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (execution_result, remainder) = FromBytes::from_bytes(bytes)?;
        let (messages, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            SpeculativeExecutionResult {
                execution_result,
                messages,
            },
            remainder,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = SpeculativeExecutionResult::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
