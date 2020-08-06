use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::Debug,
    sync::RwLock,
};

use super::{Multiple, Result, Store, Value};

/// In-memory version of a store.
#[derive(Debug)]
pub(super) struct InMemStore<V: Value> {
    inner: RwLock<HashMap<V::Id, V>>,
}

impl<V: Value> InMemStore<V> {
    pub(crate) fn new() -> Self {
        InMemStore {
            inner: RwLock::new(HashMap::new()),
        }
    }
}

impl<V: Value> Store for InMemStore<V> {
    type Value = V;

    fn put(&self, value: V) -> Result<bool> {
        if let Entry::Vacant(entry) = self.inner.write().expect("should lock").entry(*value.id()) {
            entry.insert(value.clone());
            return Ok(true);
        }
        Ok(false)
    }

    fn get(&self, ids: Multiple<V::Id>) -> Multiple<Result<Option<V>>> {
        let inner = self.inner.read().expect("should lock");
        ids.iter().map(|id| Ok(inner.get(id).cloned())).collect()
    }

    fn get_headers(&self, ids: Multiple<V::Id>) -> Multiple<Result<Option<V::Header>>> {
        let inner = self.inner.read().expect("should lock");
        ids.iter()
            .map(|id| Ok(inner.get(id).map(Value::header).cloned()))
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
