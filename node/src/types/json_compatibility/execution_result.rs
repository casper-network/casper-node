//! This file provides types to allow conversion from an EE `ExecutionResult` into a similar type
//! which can be serialized to a valid JSON representation.
//!
//! It is stored as metadata related to a given deploy, and made available to clients via the
//! JSON-RPC API.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use datasize::DataSize;
use hex_buffer_serde::{Hex, HexForm};
use lazy_static::lazy_static;
use log::info;
#[cfg(test)]
use rand::{seq::SliceRandom, Rng};
use schemars::JsonSchema;
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
    CLValue, KEY_HASH_LENGTH, U128, U256, U512,
};

use super::NamedKey;
use crate::rpcs::docs::DocExample;

#[cfg(test)]
use crate::testing::TestRng;

/// Constants to track operation serialization.
const OP_READ_TAG: u8 = 0;
const OP_WRITE_TAG: u8 = 1;
const OP_ADD_TAG: u8 = 2;
const OP_NOOP_TAG: u8 = 3;

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

lazy_static! {
    static ref EXECUTION_RESULT: ExecutionResult = {
        let mut operations = Vec::new();
        operations.push(Operation {
            key: "account-hash-2c4a11c062a8a337bfc97e27fd66291caeb2c65865dcb5d3ef3759c4c97efecb"
                .to_string(),
            kind: OpKind::Write,
        });
        operations.push(Operation {
            key: "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1"
                .to_string(),
            kind: OpKind::Read,
        });

        let mut transforms = Vec::new();
        transforms.push(TransformEntry {
            key: "account-hash-2c4a11c062a8a337bfc97e27fd66291caeb2c65865dcb5d3ef3759c4c97efecb"
                .to_string(),
            transform: Transform::AddUInt64(8u64),
        });
        transforms.push(TransformEntry {
            key: "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1"
                .to_string(),
            transform: Transform::Identity,
        });

        let effect = ExecutionEffect {
            operations,
            transforms,
        };

        let transfers = vec![
            TransferAddress([89; KEY_HASH_LENGTH]),
            TransferAddress([130; KEY_HASH_LENGTH]),
        ];

        ExecutionResult {
            effect,
            transfers,
            cost: U512::from(123_456),
            error_message: None,
        }
    };
}

impl DocExample for ExecutionResult {
    fn doc_example() -> &'static Self {
        &*EXECUTION_RESULT
    }
}

/// The hash digest; a wrapped `u8` array.
#[derive(
    Copy,
    Clone,
    DataSize,
    Ord,
    PartialOrd,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
    Default,
    Debug,
    JsonSchema,
)]
#[schemars(with = "String", description = "Hex-encoded transfer address.")]
pub struct TransferAddress(
    #[serde(with = "HexForm::<[u8; KEY_HASH_LENGTH]>")]
    #[schemars(skip, with = "String")]
    [u8; KEY_HASH_LENGTH],
);

impl ToBytes for TransferAddress {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for TransferAddress {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (address, remainder) = <[u8; KEY_HASH_LENGTH]>::from_bytes(bytes)?;
        Ok((TransferAddress(address), remainder))
    }
}

/// The result of executing a single deploy.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize, JsonSchema)]
pub struct ExecutionResult {
    effect: ExecutionEffect,
    transfers: Vec<TransferAddress>,
    cost: U512,
    error_message: Option<String>,
}

impl ExecutionResult {
    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let op_count = rng.gen_range(0, 6);
        let mut operations = Vec::new();
        for _ in 0..op_count {
            let op = [OpKind::Read, OpKind::Add, OpKind::NoOp, OpKind::Write]
                .choose(rng)
                .unwrap();
            operations.push(Operation {
                key: rng.gen::<u64>().to_string(),
                kind: *op,
            });
        }

        let transform_count = rng.gen_range(0, 6);
        let mut transforms = Vec::new();
        for _ in 0..transform_count {
            transforms.push(TransformEntry {
                key: rng.gen::<u64>().to_string(),
                transform: Transform::random(rng),
            });
        }

        let effect = ExecutionEffect {
            operations,
            transforms,
        };

        let transfer_count = rng.gen_range(0, 6);
        let mut transfers = vec![];
        for _ in 0..transfer_count {
            transfers.push(TransferAddress(rng.gen()))
        }

        let error_message = if rng.gen() {
            Some(format!("Error message {}", rng.gen::<u64>()))
        } else {
            None
        };

        ExecutionResult {
            effect,
            transfers,
            cost: rng.gen::<u64>().into(),
            error_message,
        }
    }
}

impl From<&EngineExecutionResult> for ExecutionResult {
    fn from(ee_execution_result: &EngineExecutionResult) -> Self {
        match ee_execution_result {
            EngineExecutionResult::Success {
                effect,
                transfers,
                cost,
            } => ExecutionResult {
                effect: effect.into(),
                transfers: transfers
                    .iter()
                    .map(|transfer| TransferAddress(*transfer))
                    .collect(),
                cost: cost.value(),
                error_message: None,
            },
            EngineExecutionResult::Failure {
                error,
                effect,
                transfers,
                cost,
            } => ExecutionResult {
                effect: effect.into(),
                transfers: transfers
                    .iter()
                    .map(|transfer| TransferAddress(*transfer))
                    .collect(),
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
        buffer.extend(self.transfers.to_bytes()?);
        buffer.extend(self.cost.to_bytes()?);
        buffer.extend(self.error_message.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.effect.serialized_length()
            + self.transfers.serialized_length()
            + self.cost.serialized_length()
            + self.error_message.serialized_length()
    }
}

impl FromBytes for ExecutionResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (effect, remainder) = ExecutionEffect::from_bytes(bytes)?;
        let (transfers, remainder) = Vec::<TransferAddress>::from_bytes(remainder)?;
        let (cost, remainder) = U512::from_bytes(remainder)?;
        let (error_message, remainder) = Option::<String>::from_bytes(remainder)?;
        let execution_result = ExecutionResult {
            effect,
            transfers,
            cost,
            error_message,
        };
        Ok((execution_result, remainder))
    }
}

/// The effect of executing a single deploy.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Default, Debug, DataSize, JsonSchema)]
struct ExecutionEffect {
    /// The resulting operations.
    #[data_size(skip)]
    operations: Vec<Operation>,
    /// The resulting transformations.
    #[data_size(skip)]
    transforms: Vec<TransformEntry>,
}

impl From<&EngineExecutionEffect> for ExecutionEffect {
    fn from(effect: &EngineExecutionEffect) -> Self {
        ExecutionEffect {
            operations: effect
                .ops
                .iter()
                .map(|(key, op)| Operation {
                    key: key.to_formatted_string(),
                    kind: op.into(),
                })
                .collect(),
            transforms: effect
                .transforms
                .iter()
                .map(|(key, transform)| TransformEntry {
                    key: key.to_formatted_string(),
                    transform: transform.into(),
                })
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
        let (operations, remainder) = Vec::<Operation>::from_bytes(bytes)?;
        let (transforms, remainder) = Vec::<TransformEntry>::from_bytes(remainder)?;
        let execution_effect = ExecutionEffect {
            operations,
            transforms,
        };
        Ok((execution_effect, remainder))
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize, JsonSchema)]
struct Operation {
    /// The formatted string of the `Key`.
    key: String,
    /// The type of operation.
    kind: OpKind,
}

impl ToBytes for Operation {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.key.to_bytes()?);
        buffer.extend(self.kind.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.key.serialized_length() + self.kind.serialized_length()
    }
}

impl FromBytes for Operation {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (key, remainder) = String::from_bytes(bytes)?;
        let (kind, remainder) = OpKind::from_bytes(remainder)?;
        let operation = Operation { key, kind };
        Ok((operation, remainder))
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize, JsonSchema)]
enum OpKind {
    Read,
    Write,
    Add,
    NoOp,
}

impl From<&Op> for OpKind {
    fn from(op: &Op) -> Self {
        match op {
            Op::Read => OpKind::Read,
            Op::Write => OpKind::Write,
            Op::Add => OpKind::Add,
            Op::NoOp => OpKind::NoOp,
        }
    }
}

impl ToBytes for OpKind {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        match self {
            OpKind::Read => OP_READ_TAG.to_bytes(),
            OpKind::Write => OP_WRITE_TAG.to_bytes(),
            OpKind::Add => OP_ADD_TAG.to_bytes(),
            OpKind::NoOp => OP_NOOP_TAG.to_bytes(),
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for OpKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            OP_READ_TAG => Ok((OpKind::Read, remainder)),
            OP_WRITE_TAG => Ok((OpKind::Write, remainder)),
            OP_ADD_TAG => Ok((OpKind::Add, remainder)),
            OP_NOOP_TAG => Ok((OpKind::NoOp, remainder)),
            _ => {
                info!("Failed to deserialize Operation, invalid identifier found");
                Err(bytesrepr::Error::Formatting)
            }
        }
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize, JsonSchema)]
struct TransformEntry {
    /// The formatted string of the `Key`.
    key: String,
    /// The transformation.
    transform: Transform,
}

impl ToBytes for TransformEntry {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.key.to_bytes()?);
        buffer.extend(self.transform.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.key.serialized_length() + self.transform.serialized_length()
    }
}

impl FromBytes for TransformEntry {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (key, remainder) = String::from_bytes(bytes)?;
        let (transform, remainder) = Transform::from_bytes(remainder)?;
        let transform_entry = TransformEntry { key, transform };
        Ok((transform_entry, remainder))
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize, JsonSchema)]
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
    #[data_size(skip)]
    AddKeys(Vec<NamedKey>),
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
                let mut named_keys = Vec::new();
                for _ in 0..rng.gen_range(1, 6) {
                    let _ = named_keys.push(NamedKey {
                        name: rng.gen::<u64>().to_string(),
                        key: rng.gen::<u64>().to_string(),
                    });
                }
                Transform::AddKeys(named_keys)
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
            EngineTransform::AddKeys(named_keys) => Transform::AddKeys(
                named_keys
                    .iter()
                    .map(|(name, key)| NamedKey {
                        name: name.clone(),
                        key: key.to_formatted_string(),
                    })
                    .collect(),
            ),
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
                let (value, remainder) = Vec::<NamedKey>::from_bytes(remainder)?;
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
