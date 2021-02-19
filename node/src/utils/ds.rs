//! Datasize helper functions.

use std::mem::size_of;

/// Estimate memory usage of a hashmap of keys and values each with no heap allocations.
pub fn hash_map_fixed_size<K, V>(hashmap: &std::collections::HashMap<K, V>) -> usize {
    hashmap.capacity() * (size_of::<V>() + size_of::<K>() + size_of::<usize>())
}

/// Estimate memory usage of a hashset of items with no heap allocations.
pub fn hash_set_fixed_size<T>(hashset: &std::collections::HashSet<T>) -> usize {
    hashset.capacity() * (size_of::<T>() + size_of::<usize>())
}

/// Estimate memory usage of a vec of items with no heap allocations.
pub fn vec_fixed_size<T>(vec: &Vec<T>) -> usize {
    vec.capacity() * size_of::<T>()
}
