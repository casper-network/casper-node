//! The binary port.
pub mod binary_request;
pub mod db_id;
pub mod get_request;
pub mod global_state_query_result;
pub mod non_persistent_data_request;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    contract_messages::Messages,
    execution::ExecutionResultV2,
};

/// TODO
pub struct SpeculativeExecutionResult {
    /// Result of the execution.
    pub execution_result: ExecutionResultV2,
    /// Messages emitted during execution.
    pub messages: Messages,
}

impl ToBytes for SpeculativeExecutionResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.execution_result.write_bytes(writer)?;
        self.messages.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.execution_result.serialized_length() + self.messages.serialized_length()
    }
}

impl FromBytes for SpeculativeExecutionResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (execution_result, remainder) = ExecutionResultV2::from_bytes(bytes)?;
        let (messages, remainder) = Messages::from_bytes(remainder)?;
        Ok((
            SpeculativeExecutionResult {
                execution_result,
                messages,
            },
            remainder,
        ))
    }
}
