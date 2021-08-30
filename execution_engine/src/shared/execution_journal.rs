use casper_types::{
    ExecutionEffect as JsonExecutionEffect, Key, TransformEntry as JsonTransformEntry,
};

use crate::shared::transform::Transform;

#[derive(Debug, Default, Clone, derive_more::From, derive_more::Into)]
pub struct ExecutionJournal(Vec<(Key, Transform)>);

impl From<ExecutionJournal> for JsonExecutionEffect {
    fn from(execution_journal: ExecutionJournal) -> Self {
        Self::new(
            <Vec<(Key, Transform)>>::from(execution_journal)
                .iter()
                .map(|(key, transform)| JsonTransformEntry {
                    key: key.to_formatted_string(),
                    transform: transform.into(),
                })
                .collect(),
        )
    }
}
