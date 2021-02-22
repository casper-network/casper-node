//! Datasize helper functions.

use std::{
    collections::{HashMap, HashSet},
    mem,
};

/// Estimate memory usage of a hashmap of keys and values each with no heap allocations.
pub fn hash_map_fixed_size<K, V>(hashmap: &HashMap<K, V>) -> usize {
    hashmap.capacity() * (mem::size_of::<V>() + mem::size_of::<K>() + mem::size_of::<usize>())
}

/// Estimate memory usage of a hashset of items with no heap allocations.
pub fn hash_set_fixed_size<T>(hashset: &HashSet<T>) -> usize {
    hashset.capacity() * (mem::size_of::<T>() + mem::size_of::<usize>())
}

/// Estimate memory usage of a vec of items with no heap allocations.
pub fn vec_fixed_size<T>(vec: &Vec<T>) -> usize {
    vec.capacity() * mem::size_of::<T>()
}
