use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::Debug,
    sync::RwLock,
};

use super::{DeployMetadata, DeployStore, Multiple, Result, Store, Value};
use crate::types::json_compatibility::ExecutionResult;

#[derive(Debug)]
struct ValueAndMetadata<V, M> {
    value: Option<V>,
    metadata: M,
}

impl<V, M: Default + Send + Sync> ValueAndMetadata<V, M> {
    fn from_value(value: V) -> Self {
        ValueAndMetadata {
            value: Some(value),
            metadata: M::default(),
        }
    }
}

/// In-memory version of a store.
#[derive(Debug)]
pub(super) struct InMemStore<V: Value, M> {
    inner: RwLock<HashMap<V::Id, ValueAndMetadata<V, M>>>,
}

impl<V: Value, M> InMemStore<V, M> {
    pub(crate) fn new() -> Self {
        InMemStore {
            inner: RwLock::new(HashMap::new()),
        }
    }
}

impl<V: Value, M: Default + Send + Sync> Store for InMemStore<V, M> {
    type Value = V;

    fn put(&self, value: V) -> Result<bool> {
        match self.inner.write().expect("should lock").entry(*value.id()) {
            Entry::Vacant(entry) => {
                entry.insert(ValueAndMetadata::from_value(value));
                Ok(true)
            }
            Entry::Occupied(mut entry) => {
                let data_and_metadata = entry.get_mut();
                if data_and_metadata.value.is_some() {
                    Ok(false)
                } else {
                    data_and_metadata.value = Some(value);
                    Ok(true)
                }
            }
        }
    }

    fn get(&self, ids: Multiple<V::Id>) -> Multiple<Result<Option<V>>> {
        let inner = self.inner.read().expect("should lock");
        ids.iter()
            .map(|id| Ok(inner.get(id).and_then(|entry| entry.value.clone())))
            .collect()
    }

    fn get_headers(&self, ids: Multiple<V::Id>) -> Multiple<Result<Option<V::Header>>> {
        let inner = self.inner.read().expect("should lock");
        ids.iter()
            .map(|id| {
                Ok(inner
                    .get(id)
                    .and_then(|entry| entry.value.as_ref())
                    .map(|value| value.header().clone()))
            })
            .collect()
    }

    fn ids(&self) -> Result<Vec<V::Id>> {
        Ok(self
            .inner
            .read()
            .expect("should lock")
            .keys()
            .cloned()
            .collect())
    }
}

impl<D: Value, B: Value> DeployStore for InMemStore<D, DeployMetadata<B>> {
    type Block = B;
    type Deploy = D;

    fn put_execution_result(
        &self,
        id: D::Id,
        block_hash: B::Id,
        execution_result: ExecutionResult,
    ) -> Result<bool> {
        match self.inner.write().expect("should lock").entry(id) {
            Entry::Vacant(entry) => {
                let value_and_metadata = ValueAndMetadata {
                    value: None,
                    metadata: DeployMetadata::new(block_hash, execution_result),
                };
                entry.insert(value_and_metadata);
                Ok(true)
            }
            Entry::Occupied(mut entry) => {
                let value_and_metadata = entry.get_mut();
                Ok(value_and_metadata
                    .metadata
                    .execution_results
                    .insert(block_hash, execution_result)
                    .is_none())
            }
        }
    }

    fn get_deploy_and_metadata(&self, id: D::Id) -> Result<Option<(D, DeployMetadata<B>)>> {
        Ok(self
            .inner
            .read()
            .expect("should lock")
            .get(&id)
            .and_then(|value_and_metadata| {
                value_and_metadata
                    .value
                    .as_ref()
                    .map(|value| (value.clone(), value_and_metadata.metadata.clone()))
            }))
    }
}
