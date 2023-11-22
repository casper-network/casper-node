//! The binary port.
pub mod binary_request;
pub mod db_id;
pub mod get_request;
pub mod non_persistent_data_request;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    contract_messages::Messages,
    execution::ExecutionResultV2,
    StoredValue,
};

const SUCCESS_TAG: u8 = 0;
const VALUE_NOT_FOUND_TAG: u8 = 1;
const ROOT_NOT_FOUND_TAG: u8 = 2;
const ERROR_TAG: u8 = 3;

/// Carries the result of the global state query.
pub enum GlobalStateQueryResult {
    /// Successful execution.
    Success {
        /// Stored value.
        value: StoredValue,
        /// Proof.
        merkle_proof: String,
    },
    /// Value has not been found.
    ValueNotFound,
    /// Root for the given state root hash not found.
    RootNotFound,
    /// Other error.
    Error(String),
}

impl ToBytes for GlobalStateQueryResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            GlobalStateQueryResult::Success {
                value,
                merkle_proof,
            } => {
                SUCCESS_TAG.write_bytes(writer)?;
                value.write_bytes(writer)?;
                merkle_proof.write_bytes(writer)
            }
            GlobalStateQueryResult::ValueNotFound => VALUE_NOT_FOUND_TAG.write_bytes(writer),
            GlobalStateQueryResult::RootNotFound => ROOT_NOT_FOUND_TAG.write_bytes(writer),
            GlobalStateQueryResult::Error(err) => {
                ERROR_TAG.write_bytes(writer)?;
                err.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                GlobalStateQueryResult::Success {
                    value,
                    merkle_proof,
                } => value.serialized_length() + merkle_proof.serialized_length(),
                GlobalStateQueryResult::ValueNotFound => 0,
                GlobalStateQueryResult::RootNotFound => 0,
                GlobalStateQueryResult::Error(err) => err.serialized_length(),
            }
    }
}

impl FromBytes for GlobalStateQueryResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            SUCCESS_TAG => {
                let (value, remainder) = StoredValue::from_bytes(remainder)?;
                let (merkle_proof, remainder) = String::from_bytes(remainder)?;
                Ok((
                    GlobalStateQueryResult::Success {
                        value,
                        merkle_proof,
                    },
                    remainder,
                ))
            }
            VALUE_NOT_FOUND_TAG => Ok((GlobalStateQueryResult::ValueNotFound, remainder)),
            ROOT_NOT_FOUND_TAG => Ok((GlobalStateQueryResult::RootNotFound, remainder)),
            ERROR_TAG => {
                let (error, remainder) = String::from_bytes(remainder)?;
                Ok((GlobalStateQueryResult::Error(error), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

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
