//! Datasize helper functions.

use std::{
    collections::{HashMap, HashSet},
    mem::size_of,
};

use datasize::DataSize;
use rand::{
    rngs::StdRng,
    seq::{IteratorRandom, SliceRandom},
    SeedableRng,
};

/// Number of items to sample when sampling a large collection.
const SAMPLE_SIZE: usize = 50;

/// Estimate memory usage of a hashmap of keys and values each with no heap allocations.
pub fn hash_map_fixed_size<K, V>(hashmap: &HashMap<K, V>) -> usize {
    hashmap.capacity() * (size_of::<V>() + size_of::<K>() + size_of::<usize>())
}

/// Estimate memory usage of a hashset of items with no heap allocations.
pub fn hash_set_fixed_size<T>(hashset: &HashSet<T>) -> usize {
    hashset.capacity() * (size_of::<T>() + size_of::<usize>())
}

/// Estimate memory usage of a vec of items with no heap allocations.
pub fn vec_fixed_size<T>(vec: &Vec<T>) -> usize {
    vec.capacity() * size_of::<T>()
}

/// Creates an RNG for sampling based on the length of a collection.
fn sampling_rng(len: usize) -> StdRng {
    // We use a fixed seed RNG here and hope the size will provide enough entropy to avoid gross
    // misestimations. This has the added benefit that repeated measurements will consider the
    // same nodes, reducing jitter and making this a pure function.

    // Initialize a buffer suitable for the seed, which might be larger then our length bytes.
    let mut seed = <StdRng as SeedableRng>::Seed::default();
    let len_be = len.to_be_bytes();

    // Mix in entropy from length.
    for (b1, b2) in seed.iter_mut().zip(len_be.iter()) {
        *b1 ^= *b2;
    }

    StdRng::from_seed(seed)
}

/// Given a length and a total of `sampled` bytes from sampling `SAMPLE_SIZE` items, return an
/// estimate for the total heap memory consumption of the collection.
fn scale_sample(len: usize, sampled: usize) -> usize {
    sampled * len / SAMPLE_SIZE
}

/// Extrapolate memory usage of a Vec by from a random subset of `SAMPLE_SIZE` items.
pub fn vec_sample<T>(vec: &Vec<T>) -> usize
where
    T: DataSize,
{
    if vec.len() < SAMPLE_SIZE {
        vec.estimate_heap_size()
    } else {
        let mut rng = sampling_rng(vec.len());
        let sampled = vec
            .as_slice()
            .choose_multiple(&mut rng, SAMPLE_SIZE)
            .map(DataSize::estimate_heap_size)
            .sum();
        scale_sample(vec.len(), sampled)
    }
}

/// Extrapolate memory usage of a HashMap by from a random subset of `SAMPLE_SIZE` items.
pub fn hashmap_sample<K, V>(map: &HashMap<K, V>) -> usize
where
    K: DataSize,
    V: DataSize,
{
    if map.len() < SAMPLE_SIZE {
        map.estimate_heap_size()
    } else {
        let mut rng = sampling_rng(map.len());

        let sampled = map
            .values()
            .choose_multiple(&mut rng, SAMPLE_SIZE)
            .into_iter()
            .map(DataSize::estimate_heap_size)
            .sum();

        scale_sample(map.len(), sampled)
    }
}
