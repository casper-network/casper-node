use crate::{
    abi::{CasperABI, Declaration, Definition, Definitions, StructField},
    casper::{self, read_into_vec},
    prelude::{cmp::Ordering, marker::PhantomData},
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};

use casper_executor_wasm_common::keyspace::Keyspace;

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
#[borsh(crate = "crate::serializers::borsh")]
pub struct Vector<T> {
    pub(crate) prefix: String,
    pub(crate) length: u64,
    pub(crate) _marker: PhantomData<T>,
}

impl<T: CasperABI> CasperABI for Vector<T> {
    fn populate_definitions(_definitions: &mut Definitions) {}

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

impl<T> Vector<T>
where
    T: BorshSerialize + BorshDeserialize,
{
    /// Constructs a new, empty [`Vector<T>`].
    ///
    /// The vector header will not write itself to the GS, even if
    /// values are pushed onto it later.
    pub fn new<S: Into<String>>(prefix: S) -> Self {
        Self {
            prefix: prefix.into(),
            length: 0,
            _marker: PhantomData,
        }
    }

    /// Appends an element to the back of a collection.
    pub fn push(&mut self, value: T) {
        let prefix_bytes = self.compute_prefix_bytes_for_index(self.length);
        let prefix = Keyspace::Context(&prefix_bytes);
        casper::write(prefix, &borsh::to_vec(&value).unwrap()).unwrap();
        self.length += 1;
    }

    /// Removes the last element from a vector and returns it, or None if it is empty.
    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        self.swap_remove(self.len() - 1)
    }

    /// Returns true if the slice contains an element with the given value.
    ///
    /// This operation is O(n).
    pub fn contains(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.iter().any(|v| v == *value)
    }

    /// Returns an element at index, deserialized.
    pub fn get(&self, index: u64) -> Option<T> {
        let prefix = self.compute_prefix_bytes_for_index(index);
        let item_keyspace = Keyspace::Context(&prefix);
        read_into_vec(item_keyspace)
            .unwrap()
            .map(|vec| borsh::from_slice(&vec).unwrap())
    }

    /// Returns an iterator over self, with elements deserialized.
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        (0..self.length).map(move |i| self.get(i).unwrap())
    }

    /// Inserts an element at position `index` within the vector, shifting all elements after it to
    /// the right.
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

    /// Clears the vector, removing all values from the global state.
    /// This is potentially expensive, as it requires an iteration over all elements to remove them
    /// from the global state.
    pub fn clear(&mut self) {
        for i in 0..self.length {
            let prefix_bytes = self.compute_prefix_bytes_for_index(i);
            let item_keyspace = Keyspace::Context(&prefix_bytes);
            casper::remove(item_keyspace).unwrap();
        }
        self.length = 0;
    }

    /// Returns the number of elements in the vector, also referred to as its ‘length’.
    #[inline(always)]
    pub fn len(&self) -> u64 {
        self.length
    }

    /// Returns `true` if the vector contains no elements.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Binary searches this vector for a given element. If the vector is not sorted, the returned
    /// result is unspecified and meaningless.
    pub fn binary_search(&self, value: &T) -> Result<u64, u64>
    where
        T: Ord,
    {
        self.binary_search_by(|v| v.cmp(value))
    }

    /// Binary searches this slice with a comparator function.
    ///
    /// The comparator function should return an [Ordering] that indicates whether its argument is
    /// `Less`, `Equal` or `Greater` the desired target. If the slice is not sorted or if the
    /// comparator function does not implement an order consistent with the sort order of the
    /// underlying slice, the returned result is unspecified and meaningless.
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
    ///
    /// Note: Because this shifts over the remaining elements, it has a
    /// worst-case performance of O(n). If you don’t need the order of
    /// elements to be preserved, use `swap_remove` instead.
    pub fn remove(&mut self, index: u64) -> Option<T> {
        if index >= self.length {
            return None;
        }

        let value_to_remove = self.get(index).unwrap();

        // Shift elements to the left
        for i in index..(self.length - 1) {
            if let Some(next_value) = self.get(i + 1) {
                self.write(i, next_value);
            }
        }

        // Remove the last element from storage
        self.length -= 1;
        casper::remove(Keyspace::Context(
            &self.compute_prefix_bytes_for_index(self.length),
        ))
        .unwrap();

        Some(value_to_remove)
    }

    /// Removes the element at the specified index and returns it.
    ///
    /// The removed element is replaced by the last element of the vector.
    /// This does not preserve ordering of the remaining elements, but is O(1).
    pub fn swap_remove(&mut self, index: u64) -> Option<T> {
        if index >= self.length {
            return None;
        }

        let value_to_remove = self.get(index).unwrap();
        let last_value = self.get(self.len() - 1).unwrap();

        if index != self.len() - 1 {
            self.write(index, last_value);
        }

        self.length -= 1;
        casper::remove(Keyspace::Context(
            &self.compute_prefix_bytes_for_index(self.length),
        ))
        .unwrap();

        Some(value_to_remove)
    }

    /// Retains only the elements specified by the predicate.
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        let mut i = 0;
        while i < self.length {
            if !f(&self.get(i).unwrap()) {
                self.remove(i).unwrap();
            } else {
                i += 1;
            }
        }
    }

    #[inline(always)]
    fn compute_prefix_bytes_for_index(&self, index: u64) -> Vec<u8> {
        compute_prefix_bytes_for_index(&self.prefix, index)
    }

    fn write(&self, index: u64, value: T) {
        let prefix_bytes = self.compute_prefix_bytes_for_index(index);
        let prefix = Keyspace::Context(&prefix_bytes);
        casper::write(prefix, &borsh::to_vec(&value).unwrap()).unwrap();
    }
}

fn compute_prefix_bytes_for_index(prefix: &str, index: u64) -> Vec<u8> {
    let mut prefix_bytes = prefix.as_bytes().to_owned();
    prefix_bytes.extend(&index.to_le_bytes());
    prefix_bytes
}

#[cfg(all(test, feature = "std"))]
pub(crate) mod tests {
    use core::ptr::NonNull;

    use self::casper::native::dispatch;

    use super::*;

    const TEST_VEC_PREFIX: &str = "test_vector";
    type VecU64 = Vector<u64>;

    fn get_vec_elements_from_storage(prefix: &str) -> Vec<u64> {
        let mut values = Vec::new();
        for idx in 0..64 {
            let prefix = compute_prefix_bytes_for_index(prefix, idx);
            let mut value: [u8; 8] = [0; 8];
            let result = casper::read(Keyspace::Context(&prefix), |size| {
                assert_eq!(size, 8);
                NonNull::new(value.as_mut_ptr())
            })
            .unwrap();

            if result.is_some() {
                values.push(u64::from_le_bytes(value));
            }
        }
        values
    }

    #[test]
    fn should_not_panic_with_empty_vec() {
        dispatch(|| {
            let mut vec = VecU64::new(TEST_VEC_PREFIX);
            assert_eq!(vec.len(), 0);
            assert_eq!(vec.remove(0), None);
            vec.retain(|_| false);
            let _ = vec.binary_search(&123);
            assert_eq!(
                get_vec_elements_from_storage(TEST_VEC_PREFIX),
                Vec::<u64>::new()
            );
        })
        .unwrap();
    }

    #[test]
    fn should_retain() {
        dispatch(|| {
            let mut vec = VecU64::new(TEST_VEC_PREFIX);

            vec.push(1);
            vec.push(2);
            vec.push(3);
            vec.push(4);
            vec.push(5);

            vec.retain(|v| *v % 2 == 0);

            let vec: Vec<_> = vec.iter().collect();
            assert_eq!(vec, vec![2, 4]);

            assert_eq!(get_vec_elements_from_storage(TEST_VEC_PREFIX), vec![2, 4]);
        })
        .unwrap();
    }

    #[test]
    fn test_vec() {
        dispatch(|| {
            let mut vec = VecU64::new(TEST_VEC_PREFIX);

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

            assert_eq!(
                get_vec_elements_from_storage(TEST_VEC_PREFIX),
                vec![41, 43, 42, 111, 222, 333]
            );

            let vec2 = VecU64::new("test1");
            assert_eq!(vec2.get(0), None);

            assert_eq!(get_vec_elements_from_storage("test1"), Vec::<u64>::new());
        })
        .unwrap();
    }

    #[test]
    fn test_pop() {
        dispatch(|| {
            let mut vec = VecU64::new(TEST_VEC_PREFIX);
            assert_eq!(vec.pop(), None);
            vec.push(1);
            vec.push(2);
            assert_eq!(vec.pop(), Some(2));
            assert_eq!(vec.len(), 1);
            assert_eq!(vec.pop(), Some(1));
            assert!(vec.is_empty());

            assert_eq!(
                get_vec_elements_from_storage(TEST_VEC_PREFIX),
                Vec::<u64>::new()
            );
        })
        .unwrap();
    }

    #[test]
    fn test_contains() {
        dispatch(|| {
            let mut vec = VecU64::new(TEST_VEC_PREFIX);
            vec.push(1);
            vec.push(2);
            assert!(vec.contains(&1));
            assert!(vec.contains(&2));
            assert!(!vec.contains(&3));
            vec.remove(0);
            assert!(!vec.contains(&1));
            assert_eq!(get_vec_elements_from_storage(TEST_VEC_PREFIX), vec![2]);
        })
        .unwrap();
    }

    #[test]
    fn test_clear() {
        dispatch(|| {
            let mut vec = VecU64::new(TEST_VEC_PREFIX);
            vec.push(1);
            vec.push(2);
            vec.clear();
            assert_eq!(vec.len(), 0);
            assert!(vec.is_empty());
            assert_eq!(vec.get(0), None);
            vec.push(3);
            assert_eq!(vec.get(0), Some(3));

            assert_eq!(get_vec_elements_from_storage(TEST_VEC_PREFIX), vec![3]);
        })
        .unwrap();
    }

    #[test]
    fn test_binary_search() {
        dispatch(|| {
            let mut vec = VecU64::new(TEST_VEC_PREFIX);
            vec.push(1);
            vec.push(2);
            vec.push(3);
            vec.push(4);
            vec.push(5);
            assert_eq!(vec.binary_search(&3), Ok(2));
            assert_eq!(vec.binary_search(&0), Err(0));
            assert_eq!(vec.binary_search(&6), Err(5));
        })
        .unwrap();
    }

    #[test]
    fn test_swap_remove() {
        dispatch(|| {
            let mut vec = VecU64::new(TEST_VEC_PREFIX);
            vec.push(1);
            vec.push(2);
            vec.push(3);
            vec.push(4);
            assert_eq!(vec.swap_remove(1), Some(2));
            assert_eq!(vec.iter().collect::<Vec<_>>(), vec![1, 4, 3]);
            assert_eq!(vec.swap_remove(2), Some(3));
            assert_eq!(vec.iter().collect::<Vec<_>>(), vec![1, 4]);

            assert_eq!(get_vec_elements_from_storage(TEST_VEC_PREFIX), vec![1, 4]);
        })
        .unwrap();
    }

    #[test]
    fn test_insert_at_len() {
        dispatch(|| {
            let mut vec = VecU64::new(TEST_VEC_PREFIX);
            vec.push(1);
            vec.insert(1, 2);
            assert_eq!(vec.iter().collect::<Vec<_>>(), vec![1, 2]);
            assert_eq!(get_vec_elements_from_storage(TEST_VEC_PREFIX), vec![1, 2]);
        })
        .unwrap();
    }

    #[test]
    fn test_struct_elements() {
        #[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug)]
        struct TestStruct {
            field: u64,
        }

        dispatch(|| {
            let mut vec = Vector::new(TEST_VEC_PREFIX);
            vec.push(TestStruct { field: 1 });
            vec.push(TestStruct { field: 2 });
            assert_eq!(vec.get(1), Some(TestStruct { field: 2 }));
        })
        .unwrap();
    }

    #[test]
    fn test_multiple_operations() {
        dispatch(|| {
            let mut vec = VecU64::new(TEST_VEC_PREFIX);
            assert!(vec.is_empty());
            vec.push(1);
            vec.insert(0, 2);
            vec.push(3);
            assert_eq!(vec.iter().collect::<Vec<_>>(), vec![2, 1, 3]);
            assert_eq!(vec.swap_remove(0), Some(2));
            assert_eq!(vec.iter().collect::<Vec<_>>(), vec![3, 1]);
            assert_eq!(vec.pop(), Some(1));
            assert_eq!(vec.get(0), Some(3));
            vec.clear();
            assert!(vec.is_empty());

            assert_eq!(
                get_vec_elements_from_storage(TEST_VEC_PREFIX),
                Vec::<u64>::new()
            );
        })
        .unwrap();
    }

    #[test]
    fn test_remove_invalid_index() {
        dispatch(|| {
            let mut vec = VecU64::new(TEST_VEC_PREFIX);
            vec.push(1);
            assert_eq!(vec.remove(1), None);
            assert_eq!(vec.remove(0), Some(1));
            assert_eq!(vec.remove(0), None);
        })
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn test_insert_out_of_bounds() {
        dispatch(|| {
            let mut vec = VecU64::new(TEST_VEC_PREFIX);
            vec.insert(1, 1);
        })
        .unwrap();
    }
}
