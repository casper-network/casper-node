use std::collections::BTreeMap;

use datasize::DataSize;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};

use casper_types::{CLValue, Key, U128, U256, U512};

use crate::{
    rpcs::docs::DocExample,
    types::{
        execution_result::{
            ExecutionEffect as StoredExecutionEffect, ExecutionResult as StoredExecutionResult,
            Operation, Transform as StoredTransform,
        },
        json_compatibility::KeyValuePair,
    },
};

lazy_static! {
    static ref EXECUTION_RESULT: ExecutionResult = {
        let mut effect = ExecutionEffect::default();
        effect.operations.push(KeyValuePair::new(
            "account-hash-2c4a11c062a8a337bfc97e27fd66291caeb2c65865dcb5d3ef3759c4c97efecb"
                .to_string(),
            Operation::Write,
        ));
        effect.operations.push(KeyValuePair::new(
            "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1".to_string(),
            Operation::Read,
        ));

        effect.transforms.push(KeyValuePair::new(
            "account-hash-2c4a11c062a8a337bfc97e27fd66291caeb2c65865dcb5d3ef3759c4c97efecb"
                .to_string(),
            Transform::from(StoredTransform::AddUInt64(8u64)),
        ));
        effect.transforms.push(KeyValuePair::new(
            "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1".to_string(),
            Transform::from(StoredTransform::Identity),
        ));

        ExecutionResult {
            effect,
            cost: U512::from(0),
            error_message: None,
        }
    };
}

impl DocExample for ExecutionResult {
    fn doc_example() -> &'static Self {
        &*EXECUTION_RESULT
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize)]
enum TransformPayload {
    #[data_size(skip)]
    CLValue(CLValue),
    INT32(i32),
    U64(u64),
    U128(U128),
    U256(U256),
    U512(U512),
    #[data_size(skip)]
    Keys(Vec<KeyValuePair<String, Key>>),
    Failure(String),
}

fn get_transform_payload(map: BTreeMap<String, Key>) -> Option<TransformPayload> {
    let mut payload: Vec<KeyValuePair<String, Key>> = vec![];
    for (key, value) in map.iter() {
        payload.push(KeyValuePair::new(key.clone(), *value));
    }
    Some(TransformPayload::Keys(payload))
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize)]
pub struct Transform {
    transform: KeyValuePair<String, Option<TransformPayload>>,
}

impl From<StoredTransform> for Transform {
    fn from(transform: StoredTransform) -> Self {
        match transform {
            StoredTransform::Identity => Transform {
                transform: KeyValuePair::new("Identity".to_string(), None),
            },
            StoredTransform::WriteCLValue(value) => Transform {
                transform: KeyValuePair::new(
                    "WriteCLValue".to_string(),
                    Some(TransformPayload::CLValue(value)),
                ),
            },
            StoredTransform::WriteAccount => Transform {
                transform: KeyValuePair::new("WriteAccount".to_string(), None),
            },
            StoredTransform::WriteContractWasm => Transform {
                transform: KeyValuePair::new("WriteContractWasm".to_string(), None),
            },
            StoredTransform::WriteContract => Transform {
                transform: KeyValuePair::new("WriteContract".to_string(), None),
            },
            StoredTransform::WriteContractPackage => Transform {
                transform: KeyValuePair::new("WriteContractPackage".to_string(), None),
            },
            StoredTransform::WriteDeployInfo => Transform {
                transform: KeyValuePair::new("WriteDeployInfo".to_string(), None),
            },
            StoredTransform::WriteTransfer => Transform {
                transform: KeyValuePair::new("WriteTransfer".to_string(), None),
            },
            StoredTransform::AddInt32(value) => Transform {
                transform: KeyValuePair::new(
                    "AddInt32".to_string(),
                    Some(TransformPayload::INT32(value)),
                ),
            },
            StoredTransform::AddUInt64(value) => Transform {
                transform: KeyValuePair::new(
                    "AddUInt64".to_string(),
                    Some(TransformPayload::U64(value)),
                ),
            },
            StoredTransform::AddUInt128(value) => Transform {
                transform: KeyValuePair::new(
                    "AddUInt128".to_string(),
                    Some(TransformPayload::U128(value)),
                ),
            },
            StoredTransform::AddUInt256(value) => Transform {
                transform: KeyValuePair::new(
                    "AddUInt256".to_string(),
                    Some(TransformPayload::U256(value)),
                ),
            },
            StoredTransform::AddUInt512(value) => Transform {
                transform: KeyValuePair::new(
                    "AddUInt256".to_string(),
                    Some(TransformPayload::U512(value)),
                ),
            },
            StoredTransform::AddKeys(map) => Transform {
                transform: KeyValuePair::new("AddKeys".to_string(), get_transform_payload(map)),
            },
            StoredTransform::Failure(failure) => Transform {
                transform: KeyValuePair::new(
                    "Failure".to_string(),
                    Some(TransformPayload::Failure(failure)),
                ),
            },
        }
    }
}

/// The result of executing a single deploy.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize)]
pub struct ExecutionResult {
    /// effect
    pub effect: ExecutionEffect,
    /// cost
    pub cost: U512,
    /// error
    pub error_message: Option<String>,
}

impl ExecutionResult {
    fn new(effect: ExecutionEffect, cost: U512, error_message: Option<String>) -> Self {
        ExecutionResult {
            effect,
            cost,
            error_message,
        }
    }
}

impl From<StoredExecutionResult> for ExecutionResult {
    fn from(execution_result: StoredExecutionResult) -> Self {
        let effect = execution_result.effect.into();
        ExecutionResult::new(
            effect,
            execution_result.cost,
            execution_result.error_message,
        )
    }
}

/// The effect of executing a single deploy.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, DataSize)]
pub struct ExecutionEffect {
    /// The resulting operations.  The map's key is the formatted string of the EE `Key`.
    pub operations: Vec<KeyValuePair<String, Operation>>,
    /// The resulting operations.  The map's key is the formatted string of the EE `Key`.
    pub transforms: Vec<KeyValuePair<String, Transform>>,
}

impl Default for ExecutionEffect {
    fn default() -> Self {
        ExecutionEffect::new(vec![], vec![])
    }
}

impl ExecutionEffect {
    fn new(
        operations: Vec<KeyValuePair<String, Operation>>,
        transforms: Vec<KeyValuePair<String, Transform>>,
    ) -> Self {
        ExecutionEffect {
            operations,
            transforms,
        }
    }
}

impl From<StoredExecutionEffect> for ExecutionEffect {
    fn from(execution_effect: StoredExecutionEffect) -> Self {
        let operations = execution_effect
            .operations
            .iter()
            .map(|(key, op)| KeyValuePair::new(key.clone(), *op))
            .collect();
        let transforms = execution_effect
            .transforms
            .iter()
            .map(|(key, transform)| KeyValuePair::new(key.clone(), transform.clone().into()))
            .collect();
        ExecutionEffect::new(operations, transforms)
    }
}
