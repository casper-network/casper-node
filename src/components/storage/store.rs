use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
    sync::RwLock,
};

use serde::{de::DeserializeOwned, Serialize};

/// Trait defining the API for a value able to be held within the storage component.
pub(crate) trait Value:
    Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display
{
    type Id: Copy + Clone + Ord + PartialOrd + Eq + PartialEq + Hash + Debug + Display + Send + Sync;
    type Header: Clone + Ord + PartialOrd + Eq + PartialEq + Hash + Debug + Display + Send + Sync;

    fn id(&self) -> &Self::Id;
    fn header(&self) -> &Self::Header;
}

/// Trait defining the API for a store managed by the storage component.
pub(crate) trait Store {
    type Value: Value;
    type Error: Send;

    fn put(&self, block: Self::Value) -> bool;
    fn get(&self, id: &<Self::Value as Value>::Id) -> Result<Self::Value, Self::Error>;
    fn get_header(&self, id: &<Self::Value as Value>::Id)
        -> Option<<Self::Value as Value>::Header>;
}

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

    fn put(&self, value: V) -> bool {
        let mut inner = self.inner.write().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Entry::Vacant(entry) = inner.entry(*value.id()) {
            entry.insert(value);
            return true;
        }
        false
    }

    fn get(&self, id: &V::Id) -> Result<V, InMemError> {
        self.inner
            .read()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| InMemError::ValueNotFound)
    }

    fn get_header(&self, id: &V::Id) -> Option<V::Header> {
        self.inner
            .read()
            .unwrap()
            .get(id)
            .map(Value::header)
            .cloned()
    }
}

#[derive(Debug)]
pub(crate) enum InMemError {
    ValueNotFound,
}

impl Display for InMemError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            InMemError::ValueNotFound => write!(formatter, "value not found"),
        }
    }
}
