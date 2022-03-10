//! Tracks positive integers for keys.

use std::{
    collections::HashMap,
    hash::Hash,
    ops::{AddAssign, SubAssign},
};

use datasize::DataSize;

/// A key-counter.
///
/// Allows tracking a counter for any key `K`.
///
/// Any counter that is set to `0` will not use any memory.
#[derive(DataSize, Debug)]
pub(super) struct KeyedCounter<K>(HashMap<K, usize>);

impl<K> KeyedCounter<K> {
    /// Creates a new keyed counter.
    fn new() -> Self {
        KeyedCounter(Default::default())
    }
}

impl<K> Default for KeyedCounter<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K> KeyedCounter<K>
where
    K: Clone + Eq + Hash,
{
    /// Increases count for a specific key.
    ///
    /// Returns the new value.
    pub(super) fn inc(&mut self, key: &K) -> usize {
        match self.0.get_mut(key) {
            None => {
                self.0.insert(key.clone(), 1);
                1
            }
            Some(value) => {
                value.add_assign(1);
                *value
            }
        }
    }

    /// Decreases count for a specific key.
    ///
    /// Returns the new value.
    ///
    /// # Panics
    ///
    /// Panics if `dec` would become negative.
    pub(super) fn dec(&mut self, key: &K) -> usize {
        match self.0.get_mut(key) {
            Some(value) => {
                assert_ne!(*value, 0, "counter should never be zero in tracker");

                value.sub_assign(1);

                if *value != 0 {
                    return *value;
                }
            }
            None => panic!("tried to decrease in-flight to negative value"),
        };
        assert_eq!(self.0.remove(key), Some(0));
        0
    }
}

#[cfg(test)]
mod tests {
    use super::KeyedCounter;

    #[test]
    fn can_count_up() {
        let mut kc = KeyedCounter::new();
        assert_eq!(kc.inc(&'a'), 1);
        assert_eq!(kc.inc(&'b'), 1);
        assert_eq!(kc.inc(&'a'), 2);
    }

    #[test]
    fn can_count_down() {
        let mut kc = KeyedCounter::new();
        assert_eq!(kc.inc(&'a'), 1);
        assert_eq!(kc.inc(&'b'), 1);
        assert_eq!(kc.dec(&'a'), 0);
        assert_eq!(kc.dec(&'b'), 0);
    }

    #[test]
    #[should_panic(expected = "tried to decrease in-flight to negative value")]
    fn panics_on_underflow() {
        let mut kc = KeyedCounter::new();
        assert_eq!(kc.inc(&'a'), 1);
        assert_eq!(kc.dec(&'a'), 0);
        kc.dec(&'a');
    }

    #[test]
    #[should_panic(expected = "tried to decrease in-flight to negative value")]
    fn panics_on_immediate_underflow() {
        let mut kc = KeyedCounter::new();
        kc.dec(&'a');
    }
}
