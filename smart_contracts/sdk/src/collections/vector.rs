use crate::{
    abi::{CasperABI, Declaration, Definition, Definitions, StructField},
    host::{self, read_into_vec},
};

// use casper_macros::casper;
use crate::{
    prelude::{cmp::Ordering, marker::PhantomData},
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};
use casper_executor_wasm_common::keyspace::Keyspace;
use const_fnv1a_hash::fnv1a_hash_str_64;

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
#[borsh(crate = "crate::serializers::borsh")]
pub struct Vector<T> {
    pub(crate) prefix: String,
    pub(crate) length: u64,
    pub(crate) _marker: PhantomData<T>,
}

impl<T: CasperABI> CasperABI for Vector<T> {
    fn populate_definitions(_definitions: &mut Definitions) {
        // definitions.insert(T::declaration(), T::definition());
    }

    fn declaration() -> Declaration {
        format!("Vector<{}>", T::declaration())
    }

    fn definition() -> Definition {
        Definition::Struct {
            items: vec![
                StructField {
                    name: "prefix".into(),
                    decl: String::declaration(),
                },
                StructField {
                    name: "length".into(),
                    decl: u64::declaration(),
                },
            ],
        }
    }
}

/// Computes the prefix for a given key.
#[allow(dead_code)]
pub(crate) const fn compute_prefix(input: &str) -> [u8; 8] {
    let hash = fnv1a_hash_str_64(input);
    hash.to_le_bytes()
}

impl<T> Vector<T>
where
    T: BorshSerialize + BorshDeserialize,
{
    pub fn new<S: Into<String>>(prefix: S) -> Self {
        Self {
            prefix: prefix.into(),
            length: 0,
            _marker: PhantomData,
        }
    }

    pub fn push(&mut self, value: T) {
        let mut prefix_bytes = self.prefix.as_bytes().to_owned();
        prefix_bytes.extend(&self.length.to_le_bytes());
        let prefix = Keyspace::Context(&prefix_bytes);
        host::casper_write(prefix, &borsh::to_vec(&value).unwrap()).unwrap();
        self.length += 1;
    }

    pub fn contains(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.iter().any(|v| v == *value)
    }

    pub fn get(&self, index: u64) -> Option<T> {
        let mut prefix_bytes = self.prefix.as_bytes().to_owned();
        prefix_bytes.extend(&index.to_le_bytes());
        let prefix = Keyspace::Context(&prefix_bytes);
        read_into_vec(prefix).map(|vec| borsh::from_slice(&vec).unwrap())
    }

    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        (0..self.length).map(move |i| self.get(i).unwrap())
    }

    pub fn insert(&mut self, index: u64, value: T) {
        assert!(index <= self.length, "index out of bounds");

        // Shift elements to the right
        for i in (index..self.length).rev() {
            if let Some(src_value) = self.get(i) {
                self.write(i + 1, src_value);
            }
        }

        // Write the new value at the specified index
        self.write(index, value);

        self.length += 1;
    }

    #[inline(always)]
    pub fn len(&self) -> u64 {
        self.length
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn binary_search(&self, value: &T) -> Result<u64, u64>
    where
        T: Ord,
    {
        self.binary_search_by(|v| v.cmp(value))
    }

    pub fn binary_search_by<F>(&self, mut f: F) -> Result<u64, u64>
    where
        F: FnMut(&T) -> Ordering,
    {
        // INVARIANTS:
        // - 0 <= left <= left + size = right <= self.len()
        // - f returns Less for everything in self[..left]
        // - f returns Greater for everything in self[right..]
        let mut size = self.len();
        let mut left = 0;
        let mut right = size;
        while left < right {
            let mid = left + size / 2;

            // SAFETY: the while condition means `size` is strictly positive, so
            // `size/2 < size`. Thus `left + size/2 < left + size`, which
            // coupled with the `left + size <= self.len()` invariant means
            // we have `left + size/2 < self.len()`, and this is in-bounds.
            let cmp = f(&self.get(mid).unwrap());

            // This control flow produces conditional moves, which results in
            // fewer branches and instructions than if/else or matching on
            // cmp::Ordering.
            // This is x86 asm for u8: https://rust.godbolt.org/z/698eYffTx.
            left = if cmp == Ordering::Less { mid + 1 } else { left };
            right = if cmp == Ordering::Greater { mid } else { right };
            if cmp == Ordering::Equal {
                // SAFETY: same as the `get_unchecked` above
                assert!(mid < self.len());
                return Ok(mid);
            }

            size = right - left;
        }

        // SAFETY: directly true from the overall invariant.
        // Note that this is `<=`, unlike the assume in the `Ok` path.
        assert!(left <= self.len());
        Err(left)
    }

    /// Removes the element at the specified index and returns it.
    pub fn remove(&mut self, index: u64) -> Option<T> {
        if index >= self.length {
            return None;
        }

        let value = self.get(index).unwrap();

        // Shift elements to the left
        for i in index..self.length - 1 {
            let src_value = self.get(i + 1).unwrap();
            self.write(i, src_value);
        }

        self.length -= 1;

        Some(value)
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        let mut i = 0;
        while i < self.length {
            if !f(&self.get(i).unwrap()) {
                self.remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn get_prefix_bytes(&self, index: u64) -> Vec<u8> {
        let mut prefix_bytes = self.prefix.as_bytes().to_owned();
        prefix_bytes.extend(&index.to_le_bytes());
        prefix_bytes
    }

    fn write(&self, index: u64, value: T) {
        let prefix_bytes = self.get_prefix_bytes(index);
        let prefix = Keyspace::Context(&prefix_bytes);
        host::casper_write(prefix, &borsh::to_vec(&value).unwrap()).unwrap();
    }
}

#[cfg(all(test, feature = "std"))]
pub(crate) mod tests {
    use self::host::native::dispatch;

    use super::*;

    #[test]
    fn should_not_panic_with_empty_vec() {
        dispatch(|| {
            let mut vec = Vector::<u64>::new("test");
            assert_eq!(vec.len(), 0);
            assert_eq!(vec.remove(0), None);
            vec.retain(|_| false);
            let _ = vec.binary_search(&123);
        })
        .unwrap();
    }

    #[test]
    fn should_retain() {
        dispatch(|| {
            let mut vec = Vector::<u64>::new("test");

            vec.push(1);
            vec.push(2);
            vec.push(3);
            vec.push(4);
            vec.push(5);

            vec.retain(|v| *v % 2 == 0);

            let vec: Vec<_> = vec.iter().collect();
            assert_eq!(vec, vec![2, 4]);
        })
        .unwrap();
    }

    #[test]
    fn test_vec() {
        dispatch(|| {
            let mut vec = Vector::<u64>::new("test");

            assert!(vec.get(0).is_none());
            vec.push(111);
            assert_eq!(vec.get(0), Some(111));
            vec.push(222);
            assert_eq!(vec.get(1), Some(222));

            vec.insert(0, 42);
            vec.insert(0, 41);
            vec.insert(1, 43);
            vec.insert(5, 333);
            vec.insert(5, 334);
            assert_eq!(vec.remove(5), Some(334));
            assert_eq!(vec.remove(55), None);

            let mut iter = vec.iter();
            assert_eq!(iter.next(), Some(41));
            assert_eq!(iter.next(), Some(43));
            assert_eq!(iter.next(), Some(42));
            assert_eq!(iter.next(), Some(111));
            assert_eq!(iter.next(), Some(222));
            assert_eq!(iter.next(), Some(333));
            assert_eq!(iter.next(), None);

            {
                let ser = borsh::to_vec(&vec).unwrap();
                let deser: Vector<u64> = borsh::from_slice(&ser).unwrap();
                let mut iter = deser.iter();
                assert_eq!(iter.next(), Some(41));
                assert_eq!(iter.next(), Some(43));
                assert_eq!(iter.next(), Some(42));
                assert_eq!(iter.next(), Some(111));
                assert_eq!(iter.next(), Some(222));
                assert_eq!(iter.next(), Some(333));
                assert_eq!(iter.next(), None);
            }

            let vec2 = Vector::<u64>::new("test1");
            assert_eq!(vec2.get(0), None);
        })
        .unwrap();
    }
}
