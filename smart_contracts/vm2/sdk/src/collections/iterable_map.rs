use crate::prelude::{marker::PhantomData, *};

use borsh::{BorshDeserialize, BorshSerialize};
use bytes::BufMut;
use casper_executor_wasm_common::keyspace::Keyspace;
use const_fnv1a_hash::fnv1a_hash_64;

use crate::casper::{self, read_into_vec};

/// A pointer that uniquely identifies a value written into the map.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq)]
pub struct IterableMapPtr {
    /// The key hash
    pub(crate) hash: u64,
    /// In case of a collision, signifies the index of this element
    /// in a bucket
    pub(crate) index: u64,
}

/// Trait for types that can be used as keys in [IterableMap].
/// Must produce a deterministic hash.
///
/// A blanket implementation is provided for all types that implement
/// [BorshSerialize].
pub trait IterableMapHash: PartialEq + BorshSerialize + BorshDeserialize {
    fn compute_hash(&self) -> u64 {
        let mut bytes = Vec::new();
        self.serialize(&mut bytes).unwrap();
        fnv1a_hash_64(&bytes, None)
    }
}

// No blanket IterableMapKey implementation. Explicit impls prevent conflicts with
// user‑provided implementations; a blanket impl would forbid custom hashes.
impl IterableMapHash for u8 {}
impl IterableMapHash for u16 {}
impl IterableMapHash for u32 {}
impl IterableMapHash for u64 {}
impl IterableMapHash for u128 {}
impl IterableMapHash for i8 {}
impl IterableMapHash for i16 {}
impl IterableMapHash for i32 {}
impl IterableMapHash for i64 {}
impl IterableMapHash for i128 {}
impl IterableMapHash for String {}

/// A map over global state that allows iteration. Each entry at key `K_n` stores `(K_{n}, V,
/// K_{n-1})`, where `V` is the value and `K_{n-1}` is the key hash of the previous entry.
///
/// This creates a constant spatial overhead; every entry stores a pointer
/// to the one inserted before it.
///
/// Enables iteration without a guaranteed ordering; updating an existing
/// key does not affect position.
///
/// Under the hood, this is a singly-linked HashMap with linear probing for collision resolution.
/// Supports full traversal, typically in reverse-insertion order.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
#[borsh(crate = "crate::serializers::borsh")]
pub struct IterableMap<K, V> {
    pub(crate) prefix: String,

    // Keys are hashed to u128 internally, but K is preserved to enforce type safety.
    // While this map could accept arbitrary u128 keys, requiring a concrete K prevents
    // misuse and clarifies intent at the type level.
    pub(crate) tail_key_hash: Option<IterableMapPtr>,
    _marker: PhantomData<(K, V)>,
}

/// Single entry in `IterableMap`. Stores the value and the hash of the previous entry's key.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
#[borsh(crate = "crate::serializers::borsh")]
pub struct IterableMapEntry<K, V> {
    pub(crate) key: K,
    pub(crate) value: Option<V>,
    pub(crate) previous: Option<IterableMapPtr>,
}

impl<K, V> IterableMap<K, V>
where
    K: IterableMapHash,
    V: BorshSerialize + BorshDeserialize,
{
    /// Creates an empty [IterableMap] with the given prefix.
    pub fn new<S: Into<String>>(prefix: S) -> Self {
        Self {
            prefix: prefix.into(),
            tail_key_hash: None,
            _marker: PhantomData,
        }
    }

    /// Inserts a key-value pair into the map.
    ///
    /// If the map did not have this key present, `None` is returned.
    ///
    /// If the map did have this key present, the value is updated, and the old value is returned.
    ///
    /// This has an amortized complexity of O(1), with a worst-case of O(n) when running into
    /// collisions.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        // Find an address we can write to
        let (ptr, at_ptr) = self.get_writable_slot(&key);

        // Either overwrite an existing entry, or create a new one.
        let (entry_to_write, previous) = match at_ptr {
            Some(mut entry) => {
                if entry.value.is_none() {
                    // Reuse tombstone as a new insertion
                    entry.key = key;
                    entry.previous = self.tail_key_hash;
                    entry.value = Some(value);
                    self.tail_key_hash = Some(ptr);
                    (entry, None)
                } else {
                    // Overwrite an existing value
                    let old = entry.value;
                    entry.value = Some(value);
                    (entry, old)
                }
            }
            None => {
                let entry = IterableMapEntry {
                    key,
                    value: Some(value),
                    previous: self.tail_key_hash,
                };

                // Additionally, since this is a new entry, we need to update the tail
                self.tail_key_hash = Some(ptr);

                (entry, None)
            }
        };

        // Write the entry and return previous value if it exists
        let mut entry_bytes = Vec::new();
        entry_to_write.serialize(&mut entry_bytes).unwrap();

        let prefix = self.create_prefix_from_ptr(&ptr);
        let keyspace = Keyspace::Context(&prefix);
        casper::write(keyspace, &entry_bytes).unwrap();

        previous
    }

    /// Returns a value corresponding to the key.
    pub fn get(&self, key: &K) -> Option<V> {
        // If a slot is writable, it implicitly belongs the key
        let (_, at_ptr) = self.get_writable_slot(key);
        at_ptr.and_then(|entry| entry.value)
    }

    /// Removes a key from the map. Returns the associated value if the key exists.
    ///
    /// Has a worst-case runtime of O(n).
    pub fn remove(&mut self, key: &K) -> Option<V> {
        // Find the entry for the key that we're about to remove.
        let (to_remove_ptr, at_remove_ptr) = self.find_slot(key)?;

        let to_remove_prefix = self.create_prefix_from_ptr(&to_remove_ptr);
        let to_remove_context_key = Keyspace::Context(&to_remove_prefix);

        // See if the removed entry is a part of a collision resolution chain
        // by investigating its potential child.
        let to_remove_ptr_child_prefix = self.create_prefix_from_ptr(&IterableMapPtr {
            index: to_remove_ptr.index + 1,
            ..to_remove_ptr
        });
        let to_remove_ptr_child_keyspace = Keyspace::Context(&to_remove_ptr_child_prefix);

        if self.get_entry(to_remove_ptr_child_keyspace).is_some() {
            // A child exists, so we need to retain this element to maintain
            // collision resolution soundness. Instead of purging, mark as
            // tombstone.
            let tombstone = IterableMapEntry {
                value: None,
                ..at_remove_ptr
            };

            // Write the updated value
            let mut entry_bytes = Vec::new();
            tombstone.serialize(&mut entry_bytes).unwrap();
            casper::write(to_remove_context_key, &entry_bytes).unwrap();
        } else {
            // There is no child, so we can safely purge this entry entirely.
            casper::remove(to_remove_context_key).unwrap();
        }

        // Edge case when removing tail
        if self.tail_key_hash == Some(to_remove_ptr) {
            self.tail_key_hash = at_remove_ptr.previous;
            return at_remove_ptr.value;
        }

        // Scan the map, find entry to remove, join adjacent entries
        let mut current_hash = self.tail_key_hash;
        while let Some(key) = current_hash {
            let current_prefix = self.create_prefix_from_ptr(&key);
            let current_context_key = Keyspace::Context(&current_prefix);
            let mut current_entry = self.get_entry(current_context_key).unwrap();

            // If there is no previous entry, then we've finished iterating.
            //
            // This shouldn't happen, as the outer logic prevents from running
            // into such case, ie. we early exit if the entry to remove doesn't
            // exist.
            let Some(next_hash) = current_entry.previous else {
                panic!("Unexpected end of IterableMap");
            };

            // If the next entry is the one to be removed, repoint the current
            // one to the one preceeding the one to remove.
            if next_hash == to_remove_ptr {
                // Advance current past the element to remove
                current_entry.previous = at_remove_ptr.previous;

                // Re-write the updated current entry
                let mut entry_bytes = Vec::new();
                current_entry.serialize(&mut entry_bytes).unwrap();
                casper::write(current_context_key, &entry_bytes).unwrap();

                return at_remove_ptr.value;
            }

            // Advance backwards
            current_hash = current_entry.previous;
        }

        None
    }

    /// Clears the map, removing all key-value pairs.
    pub fn clear(&mut self) {
        for key in self.keys() {
            let prefix = self.create_prefix_from_key(&key);
            {
                let key = Keyspace::Context(&prefix);
                casper::remove(key).unwrap()
            };
        }

        self.tail_key_hash = None;
    }

    /// Returns true if the map contains a value for the specified key.
    pub fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    /// Creates an iterator visiting all the values in arbitrary order.
    pub fn keys(&self) -> impl Iterator<Item = K> + '_ {
        self.iter().map(|(key, _)| key)
    }

    /// Creates an iterator visiting all the values in arbitrary order.
    pub fn values(&self) -> impl Iterator<Item = V> + '_ {
        self.iter().map(|(_, value)| value)
    }

    // Returns true if the map contains no elements.
    pub fn is_empty(&self) -> bool {
        self.tail_key_hash.is_none()
    }

    /// Returns an iterator over the entries in the map.
    ///
    /// Traverses entries in reverse-insertion order.
    /// Each item is a tuple of the hashed key and the value.
    pub fn iter(&self) -> IterableMapIter<K, V> {
        IterableMapIter {
            prefix: &self.prefix,
            current: self.tail_key_hash,
            _marker: PhantomData,
        }
    }

    /// Returns the number of entries in the map.
    ///
    /// This is an O(n) operation.
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// Find the slot containing key, if any.
    fn find_slot(&self, key: &K) -> Option<(IterableMapPtr, IterableMapEntry<K, V>)> {
        let mut bucket_ptr = self.create_root_ptr_from_key(key);

        // Probe until we find either an existing slot, a tombstone or empty space.
        // This should rarely iterate more than once assuming a solid hashing algorithm.
        loop {
            let prefix = self.create_prefix_from_ptr(&bucket_ptr);
            let keyspace = Keyspace::Context(&prefix);

            if let Some(entry) = self.get_entry(keyspace) {
                // Existing value, check if the keys match
                if entry.key == *key && entry.value.is_some() {
                    // We have found a slot where this key lives, return it
                    return Some((bucket_ptr, entry));
                } else {
                    // We found a slot for this key hash, but either the keys mismatch,
                    // or it's vacant, so we need to probe further.
                    bucket_ptr.index += 1;
                    continue;
                }
            } else {
                // We've reached empty address space, so the slot doesn't actually exist.
                return None;
            }
        }
    }

    /// Find the next slot we can safely write to. This is either a slot already owned and
    /// assigned to the key, a vacant tombstone, or empty memory.
    fn get_writable_slot(&self, key: &K) -> (IterableMapPtr, Option<IterableMapEntry<K, V>>) {
        let mut bucket_ptr = self.create_root_ptr_from_key(key);

        // Probe until we find either an existing slot, a tombstone or empty space.
        // This should rarely iterate more than once assuming a solid hashing algorithm.
        loop {
            let prefix = self.create_prefix_from_ptr(&bucket_ptr);
            let keyspace = Keyspace::Context(&prefix);

            if let Some(entry) = self.get_entry(keyspace) {
                // Existing value, check if the keys match
                if entry.key == *key {
                    // We have found an existing slot for that key, return it
                    return (bucket_ptr, Some(entry));
                } else if entry.value.is_none() {
                    // If the value is None, then this is a tombstone, and we
                    // can write over it.
                    return (bucket_ptr, Some(entry));
                } else {
                    // We found a slot for this key hash, but the keys mismatch,
                    // and it's not vacant, so this is a collision and we need to
                    // probe further.
                    bucket_ptr.index += 1;
                    continue;
                }
            } else {
                // We've reached empty address space, so we can write here
                return (bucket_ptr, None);
            }
        }
    }

    fn get_entry(&self, keyspace: Keyspace) -> Option<IterableMapEntry<K, V>> {
        match read_into_vec(keyspace) {
            Ok(Some(vec)) => {
                let entry: IterableMapEntry<K, V> = borsh::from_slice(&vec).unwrap();
                Some(entry)
            }
            Ok(None) => None,
            Err(_) => None,
        }
    }

    fn create_prefix_from_key(&self, key: &K) -> Vec<u8> {
        let ptr = self.create_root_ptr_from_key(key);
        self.create_prefix_from_ptr(&ptr)
    }

    fn create_root_ptr_from_key(&self, key: &K) -> IterableMapPtr {
        IterableMapPtr {
            hash: key.compute_hash(),
            index: 0,
        }
    }

    fn create_prefix_from_ptr(&self, hash: &IterableMapPtr) -> Vec<u8> {
        let mut context_key = Vec::new();
        context_key.extend(self.prefix.as_bytes());
        context_key.extend(b"_");
        context_key.put_u64_le(hash.hash);
        context_key.extend(b"_");
        context_key.put_u64_le(hash.index);
        context_key
    }
}

/// Iterator over entries in an [`IterableMap`].
///
/// Traverses the map in reverse-insertion order, following the internal
/// linked structure via hashed key references [`u128`].
///
/// Yields a tuple (K, V), where the key is the hashed
/// representation of the original key. The original key type `K` is not recoverable.
///
/// Each iteration step deserializes a single entry from storage.
///
/// This iterator performs no allocation beyond internal buffers,
/// and deserialization errors are treated as iteration termination.
pub struct IterableMapIter<'a, K, V> {
    prefix: &'a str,
    current: Option<IterableMapPtr>,
    _marker: PhantomData<(K, V)>,
}

impl<'a, K, V> IntoIterator for &'a IterableMap<K, V>
where
    K: BorshDeserialize,
    V: BorshDeserialize,
{
    type Item = (K, V);
    type IntoIter = IterableMapIter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        IterableMapIter {
            prefix: &self.prefix,
            current: self.tail_key_hash,
            _marker: PhantomData,
        }
    }
}

impl<K, V> Iterator for IterableMapIter<'_, K, V>
where
    K: BorshDeserialize,
    V: BorshDeserialize,
{
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        let current_hash = self.current?;
        let mut key_bytes = Vec::new();
        key_bytes.extend(self.prefix.as_bytes());
        key_bytes.extend(b"_");
        key_bytes.put_u64_le(current_hash.hash);
        key_bytes.extend(b"_");
        key_bytes.put_u64_le(current_hash.index);

        let context_key = Keyspace::Context(&key_bytes);

        match read_into_vec(context_key) {
            Ok(Some(vec)) => {
                let entry: IterableMapEntry<K, V> = borsh::from_slice(&vec).unwrap();
                self.current = entry.previous;
                Some((
                    entry.key,
                    entry
                        .value
                        .expect("Tombstone values should be unlinked on removal"),
                ))
            }
            Ok(None) => None,
            Err(_) => None,
        }
    }
}
