//! Effects that are produced as part of execution.
use casper_storage::global_state::shared::{transform::Transform, AdditiveMap};
use casper_types::Key;

use super::op::Op;
use crate::shared::execution_journal::ExecutionJournal;

/// Represents the effects of executing a single [`crate::core::engine_state::DeployItem`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecutionEffect {
    /// Operations on the keys that were used during the execution.
    pub ops: AdditiveMap<Key, Op>,
    /// Transformations on the keys that occurred during the execution of a contract. Those
    /// [`Transform`]s need to be applied in a separate commit step.
    pub transforms: AdditiveMap<Key, Transform>,
}

impl From<ExecutionJournal> for ExecutionEffect {
    fn from(journal: ExecutionJournal) -> Self {
        let mut ops = AdditiveMap::new();
        let mut transforms = AdditiveMap::new();
        for (key, transform) in journal.into_iter() {
            match transform {
                Transform::Failure(_) => (),
                Transform::Identity => ops.insert_add(key, Op::Read),
                Transform::Write(_) => ops.insert_add(key, Op::Write),
                Transform::AddInt32(_)
                | Transform::AddUInt64(_)
                | Transform::AddUInt128(_)
                | Transform::AddUInt256(_)
                | Transform::AddUInt512(_)
                | Transform::AddKeys(_) => ops.insert_add(key, Op::Add),
            };
            transforms.insert_add(key, transform);
        }

        Self { ops, transforms }
    }
}

impl From<ExecutionJournal> for AdditiveMap<Key, Transform> {
    fn from(journal: ExecutionJournal) -> Self {
        let mut transforms = AdditiveMap::new();
        for (key, transform) in journal.into_iter() {
            transforms.insert_add(key, transform);
        }
        transforms
    }
}
