use alloc::vec::Vec;

#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    execution::ExecutionResult,
    BlockHash,
};

/// The block hash and height in which a given deploy was executed, along with the execution result
/// if known.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ExecutionInfo {
    /// The hash of the block in which the deploy was executed.
    pub block_hash: BlockHash,
    /// The height of the block in which the deploy was executed.
    pub block_height: u64,
    /// The execution result if known.
    pub execution_result: Option<ExecutionResult>,
}

impl FromBytes for ExecutionInfo {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (block_hash, bytes) = FromBytes::from_bytes(bytes)?;
        let (block_height, bytes) = FromBytes::from_bytes(bytes)?;
        let (execution_result, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((
            ExecutionInfo {
                block_hash,
                block_height,
                execution_result,
            },
            bytes,
        ))
    }
}

impl ToBytes for ExecutionInfo {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut result)?;
        Ok(result)
    }

    fn write_bytes(&self, bytes: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.block_hash.write_bytes(bytes)?;
        self.block_height.write_bytes(bytes)?;
        self.execution_result.write_bytes(bytes)?;
        Ok(())
    }

    fn serialized_length(&self) -> usize {
        self.block_hash.serialized_length()
            + self.block_height.serialized_length()
            + self.execution_result.serialized_length()
    }
}
