//! Execution transformation logs.

use std::{iter::IntoIterator, vec::IntoIter};

use datasize::DataSize;

use casper_types::{
    ExecutionEffect as JsonExecutionEffect, Key, TransformEntry as JsonTransformEntry,
};

use crate::shared::transform::Transform;

/// A log of all transforms produced during execution.
#[derive(Debug, Default, Clone, Eq, PartialEq, DataSize)]
pub struct ExecutionJournal(Vec<(Key, Transform)>);

impl ExecutionJournal {
    /// Constructs a new `ExecutionJournal`.
    pub fn new(inner: Vec<(Key, Transform)>) -> Self {
        ExecutionJournal(inner)
    }

    /// Whether the journal is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many transforms are recorded in the journal.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Adds a transform to the journal.
    pub fn push(&mut self, entry: (Key, Transform)) {
        self.0.push(entry)
    }

    /// Returns an iterator over the journal entries.
    pub fn iter(&self) -> impl Iterator<Item = &(Key, Transform)> {
        self.0.iter()
    }
}

impl From<&ExecutionJournal> for JsonExecutionEffect {
    fn from(execution_journal: &ExecutionJournal) -> Self {
        Self::new(
            execution_journal
                .0
                .iter()
                .map(|(key, transform)| JsonTransformEntry {
                    key: key.to_formatted_string(),
                    transform: transform.into(),
                })
                .collect(),
        )
    }
}

impl IntoIterator for ExecutionJournal {
    type Item = (Key, Transform);
    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Extend<(Key, Transform)> for ExecutionJournal {
    fn extend<I: IntoIterator<Item = (Key, Transform)>>(&mut self, iter: I) {
        self.0.extend(iter)
    }
}
