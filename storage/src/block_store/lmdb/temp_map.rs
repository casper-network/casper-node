use std::collections::BTreeMap;

enum EntryState<V> {
    Deleted,
    Occupied(V),
}

/// A wrapper over a BTreeMap that stores changes to the backing map only temporarily.
/// The backing map will not be altered until the temporary changes are committed.
pub(crate) struct TempMap<'a, K, V: 'a> {
    base_index: &'a mut BTreeMap<K, V>,
    new_index: BTreeMap<K, EntryState<V>>,
}

impl<'a, K, V> TempMap<'a, K, V>
where
    K: Ord,
    V: 'a + Copy,
{
    /// Creates a new temporary map that is backed by a BTreeMap
    pub(crate) fn new(base_index: &'a mut BTreeMap<K, V>) -> Self {
        Self {
            base_index,
            new_index: BTreeMap::<K, EntryState<V>>::new(),
        }
    }

    /// Reads the value contained in the map at the specified key.
    pub(crate) fn get(&self, key: &K) -> Option<V> {
        if let Some(state) = self.new_index.get(key) {
            match state {
                EntryState::Occupied(val) => Some(*val),
                EntryState::Deleted => None,
            }
        } else {
            self.base_index.get(key).copied()
        }
    }

    /// Checks if a key exists in this map.
    pub(crate) fn contains_key(&self, key: &K) -> bool {
        if self.new_index.contains_key(key) {
            true
        } else {
            self.base_index.contains_key(key)
        }
    }

    /// Sets the value at the specified key index.
    pub(crate) fn insert(&mut self, key: K, val: V) {
        self.new_index.insert(key, EntryState::Occupied(val));
    }

    /// Removes the value from the map.
    pub(crate) fn remove(&mut self, key: K) {
        if self.contains_key(&key) {
            self.new_index.insert(key, EntryState::Deleted);
        }
    }

    /// Saves temporary changes to the backing map.
    pub(crate) fn commit(self) {
        for (key, val) in self.new_index {
            match val {
                EntryState::Occupied(val) => self.base_index.insert(key, val),
                EntryState::Deleted => self.base_index.remove(&key),
            };
        }
    }
}
