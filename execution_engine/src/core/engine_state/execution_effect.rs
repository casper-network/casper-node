//! Effects that are produced as part of execution.
use casper_types::Key;

use super::op::Op;
use crate::shared::{additive_map::AdditiveMap, transform::Transform};

/// Represents the effects of executing a single [`crate::core::engine_state::DeployItem`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecutionEffect {
    /// Operations on the keys that were used during the execution.
    pub ops: AdditiveMap<Key, Op>,
    /// Transformations on the keys that occurred during the execution of a contract. Those
    /// [`Transform`]s need to be applied in a separate commit step.
    pub transforms: AdditiveMap<Key, Transform>,
}

impl ExecutionEffect {
    /// Creates a new [`ExecutionEffect`].
    pub fn new(ops: AdditiveMap<Key, Op>, transforms: AdditiveMap<Key, Transform>) -> Self {
        ExecutionEffect { ops, transforms }
    }
}

impl From<&ExecutionEffect> for casper_types::ExecutionEffect {
    fn from(effect: &ExecutionEffect) -> Self {
        casper_types::ExecutionEffect {
            operations: effect
                .ops
                .iter()
                .map(|(key, op)| casper_types::Operation {
                    key: key.to_formatted_string(),
                    kind: op.into(),
                })
                .collect(),
            transforms: effect
                .transforms
                .iter()
                .map(|(key, transform)| casper_types::TransformEntry {
                    key: key.to_formatted_string(),
                    transform: transform.into(),
                })
                .collect(),
        }
    }
}
