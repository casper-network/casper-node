use crate::{
    casper::{self, read_into_vec},
    compat::types::{CLType, CLTyped},
    log,
    prelude::{borrow::ToOwned, cmp::Ordering, marker::PhantomData, Box, String, Vec},
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};

use casper_executor_wasm_common::{
    keyspace::{CollectionAddrInner, CollectionTypeTag, ContextAddr, Keyspace},
    type_uid::{TypeUid, Uid},
};
use const_fnv1a_hash::fnv1a_hash_str_64;

#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
use crate::abi::{AbiDeclaration, CasperABI, Definition, StructField};

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
#[borsh(crate = "crate::serializers::borsh")]
pub struct Vector<T> {
    pub(crate) prefix: String,
    pub(crate) length: u64,
    pub(crate) _marker: PhantomData<T>,
}

impl<T: TypeUid> TypeUid for Vector<T> {
    const UID: Uid = Uid::from_fields("Vector", &[String::UID, u64::UID, T::UID]);
}

#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
impl<T: CasperABI> CasperABI for Vector<T> {
    fn declaration() -> AbiDeclaration {
        format!("Vector<{}>", T::declaration())
    }

    fn definition() -> Definition {
        Definition::Struct {
            items: vec![
                StructField {
                    name: "prefix".into(),
                    decl: String::UID.into(),
                },
                StructField {
                    name: "length".into(),
                    decl: u64::UID.into(),
                },
            ],
        }
    }
}
impl<T: CLTyped> CLTyped for Vector<T> {
    fn cl_type() -> CLType {
        CLType::List(Box::new(T::cl_type()))
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
        let collection_prefix = fnv1a_hash_str_64(self.prefix.as_str()).to_le_bytes();
        let addr = CollectionAddrInner::new(
            *casper::get_callee().address(),
            CollectionTypeTag::Vector,
            collection_prefix,
            casper::generic_hash(&prefix_bytes, crate::types::HashAlgorithm::Blake2b).unwrap(),
        );
        casper::write(
            Keyspace::Context(ContextAddr::from(addr)),
            &borsh::to_vec(&value).unwrap(),
        )
        .unwrap();
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
        let collection_prefix = fnv1a_hash_str_64(self.prefix.as_str()).to_le_bytes();
        let addr = CollectionAddrInner::new(
            *casper::get_callee().address(),
            CollectionTypeTag::Vector,
            collection_prefix,
            casper::generic_hash(&prefix, crate::types::HashAlgorithm::Blake2b).unwrap(),
        );
        let item_keyspace = Keyspace::Context(ContextAddr::from(addr));
        log!("Foooo");
        read_into_vec(item_keyspace).unwrap().map(|vec| {
            log!("vec {:?}", vec);
            borsh::from_slice(&vec).unwrap()
        })
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
            let collection_prefix = fnv1a_hash_str_64(self.prefix.as_str()).to_le_bytes();
            let addr = CollectionAddrInner::new(
                *casper::get_callee().address(),
                CollectionTypeTag::Vector,
                collection_prefix,
                casper::generic_hash(&prefix_bytes, crate::types::HashAlgorithm::Blake2b).unwrap(),
            );
            casper::remove(Keyspace::Context(ContextAddr::from(addr))).unwrap();
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
        let addr = CollectionAddrInner::new(
            *casper::get_callee().address(),
            CollectionTypeTag::Vector,
            [0u8; 8],
            casper::generic_hash(
                &self.compute_prefix_bytes_for_index(self.length),
                crate::types::HashAlgorithm::Blake2b,
            )
            .unwrap(),
        );
        casper::remove(Keyspace::Context(ContextAddr::from(addr))).unwrap();

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
        let addr = CollectionAddrInner::new(
            *casper::get_callee().address(),
            CollectionTypeTag::Vector,
            [0u8; 8],
            casper::generic_hash(
                &self.compute_prefix_bytes_for_index(self.length),
                crate::types::HashAlgorithm::Blake2b,
            )
            .unwrap(),
        );
        casper::remove(Keyspace::Context(ContextAddr::from(addr))).unwrap();

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
        let collection_prefix = fnv1a_hash_str_64(self.prefix.as_str()).to_le_bytes();
        let addr = CollectionAddrInner::new(
            *casper::get_callee().address(),
            CollectionTypeTag::Vector,
            collection_prefix,
            casper::generic_hash(&prefix_bytes, crate::types::HashAlgorithm::Blake2b).unwrap(),
        );
        casper::write(
            Keyspace::Context(ContextAddr::from(addr)),
            &borsh::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
}

fn compute_prefix_bytes_for_index(prefix: &str, index: u64) -> Vec<u8> {
    let mut prefix_bytes = prefix.as_bytes().to_owned();
    prefix_bytes.extend(&index.to_le_bytes());
    prefix_bytes
}
