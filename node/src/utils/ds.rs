//! Datasize helper functions.

use std::{collections::HashMap, mem};

use datasize::DataSize;
use once_cell::sync::OnceCell;
use rand::{
    rngs::StdRng,
    seq::{IteratorRandom, SliceRandom},
    SeedableRng,
};

/// Number of items to sample when sampling a large collection.
const SAMPLE_SIZE: usize = 50;

/// Creates an RNG for sampling based on the length of a collection.
fn sampling_rng(len: usize) -> StdRng {
    // We use a fixed seed RNG here and hope the size will provide enough entropy to avoid gross
    // misestimations. This has the added benefit that repeated measurements will consider the
    // same nodes, reducing jitter and making this a pure function.

    // Initialize a buffer suitable for the seed, which might be larger than our length bytes.
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

/// Extrapolate memory usage of a `Vec` by from a random subset of `SAMPLE_SIZE` items.
#[allow(clippy::ptr_arg)]
pub fn vec_sample<T>(vec: &Vec<T>) -> usize
where
    T: DataSize,
{
    if vec.len() < SAMPLE_SIZE {
        vec.estimate_heap_size()
    } else {
        let base_size = vec.capacity() * mem::size_of::<T>();

        let mut rng = sampling_rng(vec.len());
        let sampled = vec
            .as_slice()
            .choose_multiple(&mut rng, SAMPLE_SIZE)
            .map(|v| v.estimate_heap_size())
            .sum();
        base_size + scale_sample(vec.len(), sampled)
    }
}

/// Extrapolate memory usage of a `HashMap` by from a random subset of `SAMPLE_SIZE` items.
pub fn hashmap_sample<K, V>(map: &HashMap<K, V>) -> usize
where
    K: DataSize,
    V: DataSize,
{
    if map.len() < SAMPLE_SIZE {
        map.estimate_heap_size()
    } else {
        let base_size =
            map.capacity() * (mem::size_of::<V>() + mem::size_of::<K>() + mem::size_of::<usize>());

        let mut rng = sampling_rng(map.len());

        let sampled = map
            .iter()
            .choose_multiple(&mut rng, SAMPLE_SIZE)
            .into_iter()
            .map(|(k, v)| k.estimate_heap_size() + v.estimate_heap_size())
            .sum();

        base_size + scale_sample(map.len(), sampled)
    }
}

pub(crate) fn once_cell<T>(cell: &OnceCell<T>) -> usize
where
    T: DataSize,
{
    cell.get().map_or(0, |value| value.estimate_heap_size())
}

#[cfg(test)]
#[allow(clippy::assertions_on_constants)] // used by sanity checks around `SAMPLE_SIZE`
mod tests {
    use std::collections::HashMap;

    use datasize::DataSize;

    use super::{hashmap_sample, vec_sample, SAMPLE_SIZE};

    #[test]
    fn vec_sample_below_sample_size() {
        let data: Vec<Box<u32>> = vec![];

        assert_eq!(vec_sample(&data), data.estimate_heap_size());

        assert!(SAMPLE_SIZE > 3);
        let data2: Vec<Box<u32>> = vec![Box::new(1), Box::new(2), Box::new(3)];

        assert_eq!(vec_sample(&data2), data2.estimate_heap_size());
    }

    #[test]
    fn vec_sample_above_sample_size() {
        let num_items = SAMPLE_SIZE * 5;

        // We make all items equal in size, so that we know the outcome of a random sampling.
        let data: Vec<Vec<u32>> = (0..num_items)
            .map(|_| vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
            .collect();

        assert_eq!(vec_sample(&data), data.estimate_heap_size());
    }

    #[test]
    fn hashmap_sample_below_sample_size() {
        let data: HashMap<u32, Box<u32>> = HashMap::new();

        assert_eq!(hashmap_sample(&data), data.estimate_heap_size());

        assert!(SAMPLE_SIZE > 3);
        let mut data2: HashMap<u32, Box<u32>> = HashMap::new();
        data2.insert(1, Box::new(1));
        data2.insert(2, Box::new(2));
        data2.insert(3, Box::new(3));

        assert_eq!(hashmap_sample(&data2), data2.estimate_heap_size());
    }

    #[test]
    fn hashmap_sample_above_sample_size() {
        let num_items = SAMPLE_SIZE * 5;

        // We make all items equal in size, so that we know the outcome of a random sampling.
        let data: HashMap<usize, Vec<u32>> = (0..num_items)
            .map(|idx| (idx, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]))
            .collect();

        assert_eq!(hashmap_sample(&data), data.estimate_heap_size());
    }
}
