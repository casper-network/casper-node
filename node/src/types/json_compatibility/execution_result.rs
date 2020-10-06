//! This file provides types to allow conversion from an EE `ExecutionResult` into a similar type
//! which can be serialized to a valid JSON representation.
//!
//! It is stored as metadata related to a given deploy, and made available to clients via the
//! JSON-RPC API.

use std::collections::{BTreeMap, HashMap};

use datasize::DataSize;
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
#[cfg(test)]
use casper_types::{bytesrepr, CLType};
use casper_types::{U128, U256, U512};

use super::CLValue;
#[cfg(test)]
use crate::testing::TestRng;

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
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Default, Debug, DataSize)]
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

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize)]
enum Transform {
    Identity,
    WriteCLValue(CLValue),
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

impl Transform {
    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0, 13) {
            0 => Transform::Identity,
            1 => {
                let tag = vec![rng.gen_range::<u8, _, _>(0, 13)];
                let cl_type: CLType = bytesrepr::deserialize(tag).unwrap();
                Transform::WriteCLValue(CLValue {
                    cl_type,
                    bytes: rng.gen::<u64>().to_string(),
                })
            }
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
                Transform::WriteCLValue(CLValue::from(cl_value))
            }
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
            EngineTransform::AddKeys(named_keys) => {
                Transform::AddKeys(super::convert_named_keys(named_keys))
            }
            EngineTransform::Failure(error) => Transform::Failure(error.to_string()),
        }
    }
}
