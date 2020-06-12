use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::Debug,
    sync::RwLock,
};

use super::{Error, Result, Store, Value};

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

    fn put(&self, value: V) -> Result<()> {
        if let Entry::Vacant(entry) = self.inner.write()?.entry(*value.id()) {
            entry.insert(value.clone());
        }
        Ok(())
    }

    fn get(&self, id: &V::Id) -> Result<V> {
        self.inner
            .read()?
            .get(id)
            .cloned()
            .ok_or_else(|| Error::NotFound)
    }

    fn get_header(&self, id: &V::Id) -> Result<V::Header> {
        self.inner
            .read()?
            .get(id)
            .map(Value::header)
            .cloned()
            .ok_or_else(|| Error::NotFound)
    }
}
