use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use casper_execution_engine::{
    core::engine_state::{
        execution_effect::ExecutionEffect as EngineExecutionEffect,
        execution_result::ExecutionResult as EngineExecutionResult, op::Op,
    },
    shared::{stored_value::StoredValue, transform::Transform as EngineTransform},
};
use casper_types::{bytesrepr::ToBytes, U128, U256, U512};

/// The result of executing a single deploy.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
pub struct ExecutionResult {
    effect: ExecutionEffect,
    cost: U512,
    error_message: Option<String>,
}

impl ExecutionResult {
    pub(crate) fn from(ee_execution_result: &EngineExecutionResult) -> Self {
        match ee_execution_result {
            EngineExecutionResult::Success { effect, cost } => ExecutionResult {
                effect: effect.into(),
                cost: cost.value(),
                error_message: None,
            },
            EngineExecutionResult::Failure {
                error,
                effect,
                cost,
            } => ExecutionResult {
                effect: effect.into(),
                cost: cost.value(),
                error_message: Some(error.to_string()),
            },
        }
    }
}

/// The effect of executing a single deploy.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
struct ExecutionEffect {
    /// The resulting operations.  The map's key is the formatted string of the EE `Key`.
    operations: HashMap<String, Operation>,
    /// The resulting operations.  The map's key is the formatted string of the EE `Key`.
    transforms: HashMap<String, Transform>,
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

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
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

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
enum Transform {
    Identity,
    WriteCLValue(String),
    WriteAccount,
    WriteContractWasm,
    WriteContract,
    WriteContractPackage,
    AddInt32(i32),
    AddUInt64(u64),
    AddUInt128(U128),
    AddUInt256(U256),
    AddUInt512(U512),
    AddKeys(BTreeMap<String, String>),
    Failure(String),
}

impl From<&EngineTransform> for Transform {
    fn from(transform: &EngineTransform) -> Self {
        match transform {
            EngineTransform::Identity => Transform::Identity,
            EngineTransform::Write(StoredValue::CLValue(cl_value)) => Transform::WriteCLValue(
                hex::encode(&cl_value.to_bytes().expect("should serialize")),
            ),
            EngineTransform::Write(StoredValue::Account(_)) => Transform::WriteAccount,
            EngineTransform::Write(StoredValue::ContractWasm(_)) => Transform::WriteContractWasm,
            EngineTransform::Write(StoredValue::Contract(_)) => Transform::WriteContract,
            EngineTransform::Write(StoredValue::ContractPackage(_)) => {
                Transform::WriteContractPackage
            }
            EngineTransform::AddInt32(value) => Transform::AddInt32(*value),
            EngineTransform::AddUInt64(value) => Transform::AddUInt64(*value),
            EngineTransform::AddUInt128(value) => Transform::AddUInt128(*value),
            EngineTransform::AddUInt256(value) => Transform::AddUInt256(*value),
            EngineTransform::AddUInt512(value) => Transform::AddUInt512(*value),
            EngineTransform::AddKeys(named_keys) => Transform::AddKeys(
                named_keys
                    .iter()
                    .map(|(name, key)| (name.clone(), key.to_formatted_string()))
                    .collect(),
            ),
            EngineTransform::Failure(error) => Transform::Failure(error.to_string()),
        }
    }
}
