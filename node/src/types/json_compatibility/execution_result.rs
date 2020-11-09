//! This file provides types to allow conversion from an EE `ExecutionResult` into a similar type
//! which can be serialized to a valid JSON representation.
//!
//! It is stored as metadata related to a given deploy, and made available to clients via the
//! JSON-RPC API.

use std::collections::BTreeMap;

use datasize::DataSize;
use log::info;
#[cfg(test)]
use rand::{seq::SliceRandom, Rng};
use serde::{Deserialize, Serialize};

use casper_execution_engine::{
    core::engine_state::{
        execution_effect::ExecutionEffect as EngineExecutionEffect,
        execution_result::ExecutionResult as EngineExecutionResult, op::Op,
    },
    shared::{stored_value::StoredValue, transform::Transform as EngineTransform},
};
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    CLValue, U128, U256, U512,
};

#[cfg(test)]
use crate::testing::TestRng;

/// Constants to track operation serialization.
const OP_READ_TAG: u8 = 1;
const OP_WRITE_TAG: u8 = 2;
const OP_ADD_TAG: u8 = 3;
const OP_NOOP_TAG: u8 = 4;

/// Constants to track Transform serialization.
const IDENTITY_TAG: u8 = 0;
const WRITE_CLVALUE_TAG: u8 = 1;
const WRITE_ACCOUNT_TAG: u8 = 2;
const WRITE_CONTRACT_WASM_TAG: u8 = 3;
const WRITE_CONTRACT_TAG: u8 = 4;
const WRITE_CONTRACT_PACKAGE_TAG: u8 = 5;
const WRITE_DEPLOY_INFO_TAG: u8 = 6;
const WRITE_TRANSFER_TAG: u8 = 7;
const ADD_INT32_TAG: u8 = 8;
const ADD_UINT64_TAG: u8 = 9;
const ADD_UINT128_TAG: u8 = 10;
const ADD_UINT256_TAG: u8 = 11;
const ADD_UINT512_TAG: u8 = 12;
const ADD_KEYS_TAG: u8 = 13;
const FAILURE_TAG: u8 = 14;

/// The result of executing a single deploy.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize)]
pub struct ExecutionResult {
    effect: ExecutionEffect,
    cost: U512,
    error_message: Option<String>,
}

impl ExecutionResult {
    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let mut effect = ExecutionEffect::default();
        let op_count = rng.gen_range(0, 6);
        for _ in 0..op_count {
            let op = [
                Operation::Read,
                Operation::Add,
                Operation::NoOp,
                Operation::Write,
            ]
            .choose(rng)
            .unwrap();
            effect.operations.insert(rng.gen::<u64>().to_string(), *op);
        }

        let transform_count = rng.gen_range(0, 6);
        for _ in 0..transform_count {
            effect
                .transforms
                .insert(rng.gen::<u64>().to_string(), Transform::random(rng));
        }

        let error_message = if rng.gen() {
            Some(format!("Error message {}", rng.gen::<u64>()))
        } else {
            None
        };

        ExecutionResult {
            effect,
            cost: rng.gen::<u64>().into(),
            error_message,
        }
    }
}

impl From<&EngineExecutionResult> for ExecutionResult {
    fn from(ee_execution_result: &EngineExecutionResult) -> Self {
        match ee_execution_result {
            EngineExecutionResult::Success { effect, cost, .. } => ExecutionResult {
                effect: effect.into(),
                cost: cost.value(),
                error_message: None,
            },
            EngineExecutionResult::Failure {
                error,
                effect,
                cost,
                ..
            } => ExecutionResult {
                effect: effect.into(),
                cost: cost.value(),
                error_message: Some(error.to_string()),
            },
        }
    }
}

impl ToBytes for ExecutionResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.effect.to_bytes()?);
        buffer.extend(self.cost.to_bytes()?);
        buffer.extend(self.error_message.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.effect.serialized_length()
            + self.cost.serialized_length()
            + self.error_message.serialized_length()
    }
}

impl FromBytes for ExecutionResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (effect, remainder) = ExecutionEffect::from_bytes(bytes)?;
        let (cost, remainder) = U512::from_bytes(remainder)?;
        let (error_message, remainder) = Option::<String>::from_bytes(remainder)?;
        let execution_result = ExecutionResult {
            effect,
            cost,
            error_message,
        };
        Ok((execution_result, remainder))
    }
}

/// The effect of executing a single deploy.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Default, Debug, DataSize)]
struct ExecutionEffect {
    /// The resulting operations.  The map's key is the formatted string of the EE `Key`.
    operations: BTreeMap<String, Operation>,
    /// The resulting operations.  The map's key is the formatted string of the EE `Key`.
    transforms: BTreeMap<String, Transform>,
}

impl From<&EngineExecutionEffect> for ExecutionEffect {
    fn from(effect: &EngineExecutionEffect) -> Self {
        ExecutionEffect {
            operations: effect
                .ops
                .iter()
                .map(|(key, op)| (key.to_formatted_string(), op.into()))
                .collect(),
            transforms: effect
                .transforms
                .iter()
                .map(|(key, transform)| (key.to_formatted_string(), transform.into()))
                .collect(),
        }
    }
}

impl ToBytes for ExecutionEffect {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.operations.to_bytes()?);
        buffer.extend(self.transforms.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.operations.serialized_length() + self.transforms.serialized_length()
    }
}

impl FromBytes for ExecutionEffect {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (operations, remainder) = BTreeMap::<String, Operation>::from_bytes(bytes)?;
        let (transforms, remainder) = BTreeMap::<String, Transform>::from_bytes(remainder)?;
        let execution_effect = ExecutionEffect {
            operations,
            transforms,
        };
        Ok((execution_effect, remainder))
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize)]
enum Operation {
    Read,
    Write,
    Add,
    NoOp,
}

impl From<&Op> for Operation {
    fn from(op: &Op) -> Self {
        match op {
            Op::Read => Operation::Read,
            Op::Write => Operation::Write,
            Op::Add => Operation::Add,
            Op::NoOp => Operation::NoOp,
        }
    }
}

impl ToBytes for Operation {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        match self {
            Operation::Read => OP_READ_TAG.to_bytes(),
            Operation::Write => OP_WRITE_TAG.to_bytes(),
            Operation::Add => OP_ADD_TAG.to_bytes(),
            Operation::NoOp => OP_NOOP_TAG.to_bytes(),
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for Operation {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            OP_READ_TAG => Ok((Operation::Read, remainder)),
            OP_WRITE_TAG => Ok((Operation::Write, remainder)),
            OP_ADD_TAG => Ok((Operation::Add, remainder)),
            OP_NOOP_TAG => Ok((Operation::NoOp, remainder)),
            _ => {
                info!("Failed to deserialize Operation, invalid identifier found");
                Err(bytesrepr::Error::Formatting)
            }
        }
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize)]
enum Transform {
    Identity,
    #[data_size(skip)]
    WriteCLValue(CLValue),
    WriteAccount,
    WriteContractWasm,
    WriteContract,
    WriteContractPackage,
    WriteDeployInfo,
    WriteTransfer,
    AddInt32(i32),
    AddUInt64(u64),
    AddUInt128(U128),
    AddUInt256(U256),
    AddUInt512(U512),
    AddKeys(BTreeMap<String, String>),
    Failure(String),
}

impl Transform {
    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0, 13) {
            0 => Transform::Identity,
            1 => Transform::WriteCLValue(CLValue::from_t(true).unwrap()),
            2 => Transform::WriteAccount,
            3 => Transform::WriteContractWasm,
            4 => Transform::WriteContract,
            5 => Transform::WriteContractPackage,
            6 => Transform::AddInt32(rng.gen()),
            7 => Transform::AddUInt64(rng.gen()),
            8 => Transform::AddUInt128(rng.gen::<u64>().into()),
            9 => Transform::AddUInt256(rng.gen::<u64>().into()),
            10 => Transform::AddUInt512(rng.gen::<u64>().into()),
            11 => {
                let mut map = BTreeMap::new();
                for _ in 0..rng.gen_range(1, 6) {
                    let _ = map.insert(rng.gen::<u64>().to_string(), rng.gen::<u64>().to_string());
                }
                Transform::AddKeys(map)
            }
            12 => Transform::Failure(rng.gen::<u64>().to_string()),
            _ => unreachable!(),
        }
    }
}

impl From<&EngineTransform> for Transform {
    fn from(transform: &EngineTransform) -> Self {
        match transform {
            EngineTransform::Identity => Transform::Identity,
            EngineTransform::Write(StoredValue::CLValue(cl_value)) => {
                Transform::WriteCLValue(cl_value.clone())
            }
            EngineTransform::Write(StoredValue::Account(_)) => Transform::WriteAccount,
            EngineTransform::Write(StoredValue::ContractWasm(_)) => Transform::WriteContractWasm,
            EngineTransform::Write(StoredValue::Contract(_)) => Transform::WriteContract,
            EngineTransform::Write(StoredValue::ContractPackage(_)) => {
                Transform::WriteContractPackage
            }
            EngineTransform::Write(StoredValue::Transfer(_)) => Transform::WriteTransfer,
            EngineTransform::Write(StoredValue::DeployInfo(_)) => Transform::WriteDeployInfo,
            EngineTransform::AddInt32(value) => Transform::AddInt32(*value),
            EngineTransform::AddUInt64(value) => Transform::AddUInt64(*value),
            EngineTransform::AddUInt128(value) => Transform::AddUInt128(*value),
            EngineTransform::AddUInt256(value) => Transform::AddUInt256(*value),
            EngineTransform::AddUInt512(value) => Transform::AddUInt512(*value),
            EngineTransform::AddKeys(named_keys) => {
                Transform::AddKeys(super::convert_named_keys(named_keys))
            }
            EngineTransform::Failure(error) => Transform::Failure(error.to_string()),
        }
    }
}

impl ToBytes for Transform {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            Transform::Identity => buffer.insert(0, IDENTITY_TAG),
            Transform::WriteCLValue(value) => {
                buffer.insert(0, WRITE_CLVALUE_TAG);
                buffer.extend(value.to_bytes()?);
            }
            Transform::WriteAccount => buffer.insert(0, WRITE_ACCOUNT_TAG),
            Transform::WriteContractWasm => buffer.insert(0, WRITE_CONTRACT_WASM_TAG),
            Transform::WriteContract => buffer.insert(0, WRITE_CONTRACT_TAG),
            Transform::WriteContractPackage => buffer.insert(0, WRITE_CONTRACT_PACKAGE_TAG),
            Transform::WriteDeployInfo => buffer.insert(0, WRITE_DEPLOY_INFO_TAG),
            Transform::WriteTransfer => buffer.insert(0, WRITE_TRANSFER_TAG),
            Transform::AddInt32(value) => {
                buffer.insert(0, ADD_INT32_TAG);
                buffer.extend(value.to_bytes()?);
            }
            Transform::AddUInt64(value) => {
                buffer.insert(0, ADD_UINT64_TAG);
                buffer.extend(value.to_bytes()?);
            }
            Transform::AddUInt128(value) => {
                buffer.insert(0, ADD_UINT128_TAG);
                buffer.extend(value.to_bytes()?);
            }
            Transform::AddUInt256(value) => {
                buffer.insert(0, ADD_UINT256_TAG);
                buffer.extend(value.to_bytes()?);
            }
            Transform::AddUInt512(value) => {
                buffer.insert(0, ADD_UINT512_TAG);
                buffer.extend(value.to_bytes()?);
            }
            Transform::AddKeys(value) => {
                buffer.insert(0, ADD_KEYS_TAG);
                buffer.extend(value.to_bytes()?);
            }
            Transform::Failure(value) => {
                buffer.insert(0, FAILURE_TAG);
                buffer.extend(value.to_bytes()?);
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        match self {
            Transform::WriteCLValue(value) => value.serialized_length() + U8_SERIALIZED_LENGTH,
            Transform::AddInt32(value) => value.serialized_length() + U8_SERIALIZED_LENGTH,
            Transform::AddUInt64(value) => value.serialized_length() + U8_SERIALIZED_LENGTH,
            Transform::AddUInt128(value) => value.serialized_length() + U8_SERIALIZED_LENGTH,
            Transform::AddUInt256(value) => value.serialized_length() + U8_SERIALIZED_LENGTH,
            Transform::AddUInt512(value) => value.serialized_length() + U8_SERIALIZED_LENGTH,
            Transform::AddKeys(value) => value.serialized_length() + U8_SERIALIZED_LENGTH,
            Transform::Failure(value) => value.serialized_length() + U8_SERIALIZED_LENGTH,
            _ => U8_SERIALIZED_LENGTH,
        }
    }
}

impl FromBytes for Transform {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            IDENTITY_TAG => Ok((Transform::Identity, remainder)),
            WRITE_CLVALUE_TAG => {
                let (cl_value, remainder) = CLValue::from_bytes(remainder)?;
                Ok((Transform::WriteCLValue(cl_value), remainder))
            }
            WRITE_ACCOUNT_TAG => Ok((Transform::WriteAccount, remainder)),
            WRITE_CONTRACT_WASM_TAG => Ok((Transform::WriteContractWasm, remainder)),
            WRITE_CONTRACT_TAG => Ok((Transform::WriteContract, remainder)),
            WRITE_CONTRACT_PACKAGE_TAG => Ok((Transform::WriteContractPackage, remainder)),
            WRITE_DEPLOY_INFO_TAG => Ok((Transform::WriteDeployInfo, remainder)),
            WRITE_TRANSFER_TAG => Ok((Transform::WriteTransfer, remainder)),
            ADD_INT32_TAG => {
                let (value_i32, remainder) = i32::from_bytes(remainder)?;
                Ok((Transform::AddInt32(value_i32), remainder))
            }
            ADD_UINT64_TAG => {
                let (value_u64, remainder) = u64::from_bytes(remainder)?;
                Ok((Transform::AddUInt64(value_u64), remainder))
            }
            ADD_UINT128_TAG => {
                let (value_u128, remainder) = U128::from_bytes(remainder)?;
                Ok((Transform::AddUInt128(value_u128), remainder))
            }
            ADD_UINT256_TAG => {
                let (value_u256, remainder) = U256::from_bytes(remainder)?;
                Ok((Transform::AddUInt256(value_u256), remainder))
            }
            ADD_UINT512_TAG => {
                let (value_u512, remainder) = U512::from_bytes(remainder)?;
                Ok((Transform::AddUInt512(value_u512), remainder))
            }
            ADD_KEYS_TAG => {
                let (value, remainder) = BTreeMap::<String, String>::from_bytes(remainder)?;
                Ok((Transform::AddKeys(value), remainder))
            }
            FAILURE_TAG => {
                let (value, remainder) = String::from_bytes(remainder)?;
                Ok((Transform::Failure(value), remainder))
            }
            _ => {
                info!("Failed to deserialize Transform, invalid identifier found");
                Err(bytesrepr::Error::Formatting)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_test_transform() {
        let mut rng = TestRng::new();
        let transform = Transform::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&transform);
    }

    #[test]
    fn bytesrepr_test_operation() {
        let mut rng = TestRng::new();
        let execution_result = ExecutionResult::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&execution_result.effect.operations);
    }

    #[test]
    fn bytesrepr_test_execution_effect() {
        let mut rng = TestRng::new();
        let execution_result = ExecutionResult::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&execution_result.effect);
    }

    #[test]
    fn bytesrepr_test_execution_result() {
        let mut rng = TestRng::new();
        let execution_result = ExecutionResult::random(&mut rng);
        bytesrepr::test_serialization_roundtrip(&execution_result);
    }
}
