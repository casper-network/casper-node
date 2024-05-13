use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::distributions::Distribution;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{ExecutionResultV1, ExecutionResultV2};
use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

const V1_TAG: u8 = 0;
const V2_TAG: u8 = 1;

/// The versioned result of executing a single deploy.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum ExecutionResult {
    /// Version 1 of execution result type.
    #[serde(rename = "Version1")]
    V1(ExecutionResultV1),
    /// Version 2 of execution result type.
    #[serde(rename = "Version2")]
    V2(ExecutionResultV2),
}

impl ExecutionResult {
    /// Returns a random ExecutionResult.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        if rng.gen_bool(0.5) {
            Self::V1(rand::distributions::Standard.sample(rng))
        } else {
            Self::V2(ExecutionResultV2::random(rng))
        }
    }
}

impl From<ExecutionResultV1> for ExecutionResult {
    fn from(value: ExecutionResultV1) -> Self {
        ExecutionResult::V1(value)
    }
}

impl From<ExecutionResultV2> for ExecutionResult {
    fn from(value: ExecutionResultV2) -> Self {
        ExecutionResult::V2(value)
    }
}

impl ToBytes for ExecutionResult {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            ExecutionResult::V1(result) => {
                V1_TAG.write_bytes(writer)?;
                result.write_bytes(writer)
            }
            ExecutionResult::V2(result) => {
                V2_TAG.write_bytes(writer)?;
                result.write_bytes(writer)
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
                ExecutionResult::V1(result) => result.serialized_length(),
                ExecutionResult::V2(result) => result.serialized_length(),
            }
    }
}

impl FromBytes for ExecutionResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            V1_TAG => {
                let (result, remainder) = ExecutionResultV1::from_bytes(remainder)?;
                Ok((ExecutionResult::V1(result), remainder))
            }
            V2_TAG => {
                let (result, remainder) = ExecutionResultV2::from_bytes(remainder)?;
                Ok((ExecutionResult::V2(result), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let execution_result = ExecutionResult::V1(rng.gen());
        bytesrepr::test_serialization_roundtrip(&execution_result);
        let execution_result = ExecutionResult::from(ExecutionResultV2::random(rng));
        bytesrepr::test_serialization_roundtrip(&execution_result);
    }

    #[test]
    fn bincode_roundtrip() {
        let rng = &mut TestRng::new();
        let execution_result = ExecutionResult::V1(rng.gen());
        let serialized = bincode::serialize(&execution_result).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(execution_result, deserialized);

        let execution_result = ExecutionResult::from(ExecutionResultV2::random(rng));
        let serialized = bincode::serialize(&execution_result).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(execution_result, deserialized);
    }

    #[test]
    fn json_roundtrip() {
        let rng = &mut TestRng::new();
        let execution_result = ExecutionResult::V1(rng.gen());
        let serialized = serde_json::to_string(&execution_result).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(execution_result, deserialized);

        let execution_result = ExecutionResult::from(ExecutionResultV2::random(rng));
        let serialized = serde_json::to_string(&execution_result).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(execution_result, deserialized);
    }
}
