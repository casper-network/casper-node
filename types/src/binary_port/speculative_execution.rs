//! The result of the speculative execution request.

use thiserror::Error;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    contract_messages::Messages,
    execution::ExecutionResultV2,
};

const NO_SUCH_STATE_ROOT_TAG: u8 = 0;
const INVALID_DEPLOY_TAG: u8 = 1;
const INTERNAL_ERROR_TAG: u8 = 2;

/// Error for the speculative deploy execution.
#[derive(Debug, Error)]
pub enum SpeculativeExecutionError {
    /// Specified state root not found.
    #[error("No such state root")]
    NoSuchStateRoot,
    /// The deploy is invalid.
    #[error("Invalid deploy: {}", _0)]
    InvalidDeploy(String),
    /// Internal error.
    #[error("Internal error: {}", _0)]
    InternalError(String),
}

impl ToBytes for SpeculativeExecutionError {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            SpeculativeExecutionError::NoSuchStateRoot => {
                NO_SUCH_STATE_ROOT_TAG.write_bytes(writer)
            }
            SpeculativeExecutionError::InvalidDeploy(err) => {
                INVALID_DEPLOY_TAG.write_bytes(writer)?;
                err.write_bytes(writer)
            }
            SpeculativeExecutionError::InternalError(err) => {
                INTERNAL_ERROR_TAG.write_bytes(writer)?;
                err.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                SpeculativeExecutionError::NoSuchStateRoot => 0,
                SpeculativeExecutionError::InvalidDeploy(err)
                | SpeculativeExecutionError::InternalError(err) => err.serialized_length(),
            }
    }
}

impl FromBytes for SpeculativeExecutionError {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            NO_SUCH_STATE_ROOT_TAG => Ok((SpeculativeExecutionError::NoSuchStateRoot, remainder)),
            INVALID_DEPLOY_TAG => {
                let (err, remainder) = String::from_bytes(remainder)?;
                Ok((SpeculativeExecutionError::InvalidDeploy(err), remainder))
            }
            INTERNAL_ERROR_TAG => {
                let (err, remainder) = String::from_bytes(remainder)?;
                Ok((SpeculativeExecutionError::InternalError(err), remainder))
            }
            _ => return Err(bytesrepr::Error::NotRepresentable),
        }
    }
}

/// Result of the speculative execution request.
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