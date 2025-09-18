use borsh::{BorshDeserialize, BorshSerialize};

use super::{IterableMap, IterableMapHash};

/// An iterable set backed by a map.
pub struct IterableSet<V> {
    pub(crate) map: IterableMap<V, ()>,
}

impl<V: IterableMapHash + BorshSerialize + BorshDeserialize + Clone> IterableSet<V> {
    /// Creates an empty [IterableMap] with the given prefix.
    pub fn new<S: Into<String>>(prefix: S) -> Self {
        Self {
            map: IterableMap::new(prefix),
        }
    }

    /// Inserts a value into the set.
    pub fn insert(&mut self, value: V) {
        self.map.insert(value, ());
    }

    /// Removes a value from the set.
    ///
    /// Has a worst-case runtime of O(n).
    pub fn remove(&mut self, value: &V) {
        self.map.remove(value);
    }

    /// Returns true if the set contains a value.
    pub fn contains(&self, value: &V) -> bool {
        self.map.get(value).is_some()
    }

    /// Creates an iterator visiting all the values in arbitrary order.
    pub fn iter(&self) -> impl Iterator<Item = V> + '_ {
        self.map.iter().map(|(value, _)| value)
    }

    // Returns true if the set contains no elements.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Clears the set, removing all values.
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::casper::native::dispatch;
    use borsh::{BorshDeserialize, BorshSerialize};

    #[test]
    fn basic_insert_contains() {
        dispatch(|| {
            let mut set = IterableSet::new("test_set");
            assert!(!set.contains(&1));

            set.insert(1);
            assert!(set.contains(&1));

            set.insert(2);
            assert!(set.contains(&2));
        })
        .unwrap();
    }

    #[test]
    fn remove_elements() {
        dispatch(|| {
            let mut set = IterableSet::new("test_set");
            set.insert(1);
            set.insert(2);

            set.remove(&1);
            assert!(!set.contains(&1));
            assert!(set.contains(&2));

            set.remove(&2);
            assert!(set.is_empty());
        })
        .unwrap();
    }

    #[test]
    fn iterator_order_and_contents() {
        dispatch(|| {
            let mut set = IterableSet::new("test_set");
            set.insert(1);
            set.insert(2);
            set.insert(3);

            let mut items: Vec<_> = set.iter().collect();
            items.sort();
            assert_eq!(items, vec![1, 2, 3]);
        })
        .unwrap();
    }

    #[test]
    fn clear_functionality() {
        dispatch(|| {
            let mut set = IterableSet::new("test_set");
            set.insert(1);
            set.insert(2);

            assert!(!set.is_empty());
            set.clear();
            assert!(set.is_empty());
            assert_eq!(set.iter().count(), 0);
        })
        .unwrap();
    }

    #[test]
    fn multiple_sets_independence() {
        dispatch(|| {
            let mut set1 = IterableSet::new("set1");
            let mut set2 = IterableSet::new("set2");

            set1.insert(1);
            set2.insert(1);

            assert!(set1.contains(&1));
            assert!(set2.contains(&1));

            set1.remove(&1);
            assert!(!set1.contains(&1));
            assert!(set2.contains(&1));
        })
        .unwrap();
    }

    #[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
    struct TestStruct {
        field1: u64,
        field2: String,
    }

    impl IterableMapHash for TestStruct {}

    #[test]
    fn struct_values() {
        dispatch(|| {
            let val1 = TestStruct {
                field1: 1,
                field2: "a".to_string(),
            };
            let val2 = TestStruct {
                field1: 2,
                field2: "b".to_string(),
            };

            let mut set = IterableSet::new("test_set");
            set.insert(val1.clone());
            set.insert(val2.clone());

            assert!(set.contains(&val1));
            assert!(set.contains(&val2));

            let mut collected: Vec<_> = set.iter().collect();
            collected.sort_by(|a, b| a.field1.cmp(&b.field1));
            assert_eq!(collected, vec![val1, val2]);
        })
        .unwrap();
    }

    #[test]
    fn duplicate_insertions() {
        dispatch(|| {
            let mut set = IterableSet::new("test_set");
            set.insert(1);
            set.insert(1); // Should be no-op

            assert_eq!(set.iter().count(), 1);
            set.remove(&1);
            assert!(set.is_empty());
        })
        .unwrap();
    }

    #[test]
    fn empty_set_behavior() {
        dispatch(|| {
            let set = IterableSet::<u64>::new("test_set");
            assert!(set.is_empty());
            assert_eq!(set.iter().count(), 0);

            let mut set = set;
            set.remove(&999); // Shouldn't panic
            assert!(set.is_empty());
        })
        .unwrap();
    }

    #[test]
    fn complex_operations_sequence() {
        dispatch(|| {
            let mut set = IterableSet::new("test_set");
            set.insert(1);
            set.insert(2);
            set.remove(&1);
            set.insert(3);
            set.clear();
            set.insert(4);

            let items: Vec<_> = set.iter().collect();
            assert_eq!(items, vec![4]);
        })
        .unwrap();
    }
}
