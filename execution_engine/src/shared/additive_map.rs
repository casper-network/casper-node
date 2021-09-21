//! Support for additive map that allows adds values to existing items instead of overwriting them.
use std::{
    borrow::Borrow,
    collections::{
        hash_map::{IntoIter, Iter, IterMut, Keys, RandomState, Values},
        HashMap,
    },
    fmt::{self, Debug, Formatter},
    hash::{BuildHasher, Hash},
    iter::{FromIterator, IntoIterator},
    ops::{AddAssign, Index},
};

/// An associative container that looks andgram works a normal `HashMap` except that instead of overwriting
/// existing items it allows for adding new value to existing value instead of overwriting it.
#[derive(Clone)]
pub struct AdditiveMap<K, V, S = RandomState>(HashMap<K, V, S>);

impl<K: Eq + Hash, V> AdditiveMap<K, V, RandomState> {
    /// Creates an empty `AdditiveMap`.
    pub fn new() -> Self {
        Self(Default::default())
    }
}

impl<K: Eq + Hash, V: AddAssign + Default, S: BuildHasher> AdditiveMap<K, V, S> {
    /// Modifies the existing value stored under `key`, or the default value for `V` if none, by
    /// adding `value_to_add`.
    pub fn insert_add(&mut self, key: K, value_to_add: V) {
        let current_value = self.0.entry(key).or_default();
        *current_value += value_to_add;
    }
}

impl<K, V, S> AdditiveMap<K, V, S> {
    /// An iterator visiting all keys in arbitrary order. The iterator element type is `&'a K`.
    pub fn keys(&self) -> Keys<'_, K, V> {
        self.0.keys()
    }

    /// An iterator visiting all values in arbitrary order. The iterator element type is `&'a V`.
    pub fn values(&self) -> Values<'_, K, V> {
        self.0.values()
    }

    /// An iterator visiting all key-value pairs in arbitrary order. The iterator element type is
    /// `(&'a K, &'a V)`.
    pub fn iter(&self) -> Iter<'_, K, V> {
        self.0.iter()
    }

    /// Returns the number of elements in the map.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the map contains no elements.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<K: Eq + Hash, V, S: BuildHasher> AdditiveMap<K, V, S> {
    /// Returns a reference to the value corresponding to the key.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.0.get(key)
    }

    /// Inserts a key-value pair into the map.
    ///
    /// If the map did not have this key present, None is returned.
    /// If the map did have this key present, the value is updated, and the old value is returned.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.0.insert(key, value)
    }

    /// Removes a key from the map, returning the value at the key if the key was previously in the map.
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.0.remove(key)
    }
    
    /// Removes a key from the map, returning the stored key and value if the key was previously in the map.
    pub fn remove_entry<Q>(&mut self, key: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.0.remove_entry(key)
    }
}

impl<K: Eq + Hash, V, S: BuildHasher + Default> Default for AdditiveMap<K, V, S> {
    fn default() -> Self {
        Self(HashMap::with_hasher(Default::default()))
    }
}

impl<'a, K, V, S> IntoIterator for &'a AdditiveMap<K, V, S> {
    type Item = (&'a K, &'a V);
    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Iter<'a, K, V> {
        self.0.iter()
    }
}

impl<'a, K, V, S> IntoIterator for &'a mut AdditiveMap<K, V, S> {
    type Item = (&'a K, &'a mut V);
    type IntoIter = IterMut<'a, K, V>;

    fn into_iter(self) -> IterMut<'a, K, V> {
        self.0.iter_mut()
    }
}

impl<K, V, S> IntoIterator for AdditiveMap<K, V, S> {
    type Item = (K, V);
    type IntoIter = IntoIter<K, V>;

    fn into_iter(self) -> IntoIter<K, V> {
        self.0.into_iter()
    }
}

impl<K: Eq + Hash, V, S: BuildHasher + Default> FromIterator<(K, V)> for AdditiveMap<K, V, S> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        Self(HashMap::from_iter(iter))
    }
}

impl<K, Q, V, S> Index<&Q> for AdditiveMap<K, V, S>
where
    K: Eq + Hash + Borrow<Q>,
    Q: Eq + Hash + ?Sized,
    S: BuildHasher,
{
    type Output = V;

    fn index(&self, key: &Q) -> &V {
        &self.0[key]
    }
}

impl<K: Eq + Hash, V: PartialEq, S: BuildHasher> PartialEq for AdditiveMap<K, V, S> {
    fn eq(&self, other: &AdditiveMap<K, V, S>) -> bool {
        self.0 == other.0
    }
}

impl<K: Eq + Hash, V: Eq, S: BuildHasher> Eq for AdditiveMap<K, V, S> {}

impl<K: Eq + Hash + Debug, V: Debug, S: BuildHasher> Debug for AdditiveMap<K, V, S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::AdditiveMap;
    use crate::shared::transform::Transform;

    #[test]
    fn insert_add() {
        let key = "key";
        let mut int_map = AdditiveMap::new();
        int_map.insert_add(key, 1);
        assert_eq!(1, int_map[key]);
        int_map.insert_add(key, 2);
        assert_eq!(3, int_map[key]);

        let mut transform_map = AdditiveMap::new();
        transform_map.insert_add(key, Transform::AddUInt64(1));
        assert_eq!(Transform::AddUInt64(1), transform_map[key]);
        transform_map.insert_add(key, Transform::AddInt32(2));
        assert_eq!(Transform::AddInt32(3), transform_map[key]);
    }
}
