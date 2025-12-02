use core::{panic, ptr::NonNull};

use alloc::{vec, vec::Vec};
use casper_contract_sdk::{
    casper,
    collections::{compute_prefix_bytes_for_index, Vector},
    types::HashAlgorithm,
};
use casper_executor_wasm_common::{
    keyspace::{CollectionAddrInner, ContextAddr, Keyspace},
    CollectionTypeTag,
};
use const_fnv1a_hash::fnv1a_hash_str_64;

use crate::types::VectorTestStruct;

fn get_vec_elements_from_storage(prefix: &str) -> Vec<u64> {
    let mut values = Vec::new();
    for idx in 0..64 {
        let prefix_bytes = compute_prefix_bytes_for_index(prefix, idx);
        let collection_prefix = fnv1a_hash_str_64(prefix).to_le_bytes();
        let addr = CollectionAddrInner::new(
            *casper::get_callee().address(),
            CollectionTypeTag::Vector,
            collection_prefix,
            casper::generic_hash(&prefix_bytes, HashAlgorithm::Blake2b).unwrap(),
        );

        let mut value: [u8; 8] = [0; 8];
        let result = casper::read(Keyspace::Context(ContextAddr::from(addr)), |size| {
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

pub(crate) fn should_not_panic_with_empty_assert(vec: &mut Vector<u64>) {
    assert_eq!(vec.len(), 0);
    assert_eq!(vec.remove(0), None);
    vec.retain(|_| false);
    let _ = vec.binary_search(&123);
    let v: Vec<u64> = vec.iter().collect();
    assert_eq!(v, Vec::<u64>::new())
}

pub(crate) fn should_retain_assert(vec: &mut Vector<u64>) {
    let vec: Vec<_> = vec.iter().collect();
    assert_eq!(vec, vec![2, 4]);
}

pub(crate) fn test_vec_assert(vec: &mut Vector<u64>) {
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
        let ser = borsh::to_vec(vec).unwrap();
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
        get_vec_elements_from_storage("test_vec"),
        vec![41, 43, 42, 111, 222, 333]
    );

    let vec2 = Vector::<u64>::new("test1");
    assert_eq!(vec2.get(0), None);

    assert_eq!(get_vec_elements_from_storage("test1"), Vec::<u64>::new());
}

pub(crate) fn test_pop_assert(vec: &mut Vector<u64>) {
    assert_eq!(vec.pop(), Some(2));
    assert_eq!(vec.len(), 1);
    assert_eq!(vec.pop(), Some(1));
    assert!(vec.is_empty());

    assert_eq!(get_vec_elements_from_storage("test_pop"), Vec::<u64>::new());
}

pub(crate) fn test_contains_assert(vec: &mut Vector<u64>) {
    assert!(vec.contains(&1));
    assert!(vec.contains(&2));
    assert!(!vec.contains(&3));
    vec.remove(0);
    assert!(!vec.contains(&1));
    assert_eq!(
        get_vec_elements_from_storage("test_contains_prepare"),
        vec![2]
    );
}

pub(crate) fn test_clear_assert(vec: &mut Vector<u64>) {
    vec.clear();
    assert_eq!(vec.len(), 0);
    assert!(vec.is_empty());
    assert_eq!(vec.get(0), None);
    vec.push(3);
    assert_eq!(vec.get(0), Some(3));

    assert_eq!(get_vec_elements_from_storage("test_clear"), vec![3]);
}

pub(crate) fn test_binary_search_assert(vec: &mut Vector<u64>) {
    assert_eq!(vec.binary_search(&3), Ok(2));
    assert_eq!(vec.binary_search(&0), Err(0));
    assert_eq!(vec.binary_search(&6), Err(5));
}

pub(crate) fn test_swap_remove_assert(vec: &mut Vector<u64>) {
    assert_eq!(vec.iter().collect::<Vec<_>>(), vec![1, 4]);
    assert_eq!(
        get_vec_elements_from_storage("test_swap_remove"),
        vec![1, 4]
    );
}

pub(crate) fn test_insert_at_len_assert(vec: &mut Vector<u64>) {
    assert_eq!(vec.iter().collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(
        get_vec_elements_from_storage("test_insert_at_len"),
        vec![1, 2]
    );
}

pub(crate) fn test_struct_elements_assert(vec: &mut Vector<VectorTestStruct>) {
    assert_eq!(vec.get(1), Some(VectorTestStruct { field: 2 }));
    assert_eq!(vec.len(), 2);
}

pub(crate) fn test_multiple_operations_assert(vec: &mut Vector<u64>) {
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
        get_vec_elements_from_storage("test_multiple_operations"),
        Vec::<u64>::new()
    );
}

pub(crate) fn test_remove_invalid_index_assert(vec: &mut Vector<u64>) {
    assert_eq!(vec.remove(1), None);
    assert_eq!(vec.remove(0), Some(1));
    assert_eq!(vec.remove(0), None);
}
