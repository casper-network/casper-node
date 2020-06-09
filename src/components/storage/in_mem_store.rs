use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::Debug,
    sync::RwLock,
};

use super::{InMemError, InMemResult, Store, Value};

/// In-memory version of a store.
#[derive(Debug)]
pub(crate) struct InMemStore<V: Value> {
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
    type Error = InMemError;

    fn put(&self, value: V) -> InMemResult<()> {
        if let Entry::Vacant(entry) = self.inner.write()?.entry(*value.id()) {
            entry.insert(value);
        }
        Ok(())
    }

    fn get(&self, id: &V::Id) -> InMemResult<V> {
        self.inner
            .read()?
            .get(id)
            .cloned()
            .ok_or_else(|| InMemError::ValueNotFound)
    }

    fn get_header(&self, id: &V::Id) -> InMemResult<V::Header> {
        self.inner
            .read()?
            .get(id)
            .map(Value::header)
            .cloned()
            .ok_or_else(|| InMemError::ValueNotFound)
    }
}
