use casper_contract_sdk::{casper, collections::Vector};

use crate::types::VectorTestStruct;

pub(crate) fn should_not_panic_with_empty_prepare() -> Vector<u64> {
    let mut vec = Vector::new("should_not_panic_with_empty");
    assert_eq!(vec.len(), 0);
    assert_eq!(vec.remove(0), None);
    vec.retain(|_| false);
    let _ = vec.binary_search(&123);
    vec
}

pub(crate) fn should_retain_prepare() -> Vector<u64> {
    let mut vec = Vector::new("should_retain");
    vec.push(1);
    vec.push(2);
    vec.push(3);
    vec.push(4);
    vec.push(5);
    vec.retain(|v| *v % 2 == 0);
    vec
}

pub(crate) fn test_vec_prepare() -> Vector<u64> {
    let mut vec = Vector::new("test_vec");

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
    vec
}

pub(crate) fn test_pop_prepare() -> Vector<u64> {
    let mut vec = Vector::new("test_pop");
    assert_eq!(vec.pop(), None);
    vec.push(1);
    vec.push(2);
    vec
}

pub(crate) fn test_contains_prepare() -> Vector<u64> {
    let mut vec = Vector::new("test_contains_prepare");
    vec.push(1);
    vec.push(2);
    assert!(vec.contains(&1));
    assert!(vec.contains(&2));
    assert!(!vec.contains(&3));
    vec
}

pub(crate) fn test_clear_prepare() -> Vector<u64> {
    let mut vec = Vector::new("test_clear");
    vec.push(1);
    vec.push(2);
    vec
}

pub(crate) fn test_binary_search_prepare() -> Vector<u64> {
    let mut vec = Vector::new("test_binary_search");
    vec.push(1);
    vec.push(2);
    vec.push(3);
    vec.push(4);
    vec.push(5);
    vec
}

pub(crate) fn test_swap_remove_prepare() -> Vector<u64> {
    let mut vec = Vector::new("test_swap_remove");
    vec.push(1);
    vec.push(2);
    vec.push(3);
    vec.push(4);
    assert_eq!(vec.swap_remove(1), Some(2));
    assert_eq!(vec.swap_remove(2), Some(3));
    vec
}

pub(crate) fn test_insert_at_len_prepare() -> Vector<u64> {
    let mut vec = Vector::new("test_insert_at_len");
    vec.push(1);
    vec.insert(1, 2);
    vec
}

pub(crate) fn test_struct_elements_prepare() -> Vector<VectorTestStruct> {
    let mut vec = Vector::new("test_struct_elements");
    vec.push(VectorTestStruct { field: 1 });
    vec.push(VectorTestStruct { field: 2 });
    assert_eq!(vec.get(1), Some(VectorTestStruct { field: 2 }));
    vec
}

pub(crate) fn test_multiple_operations_prepare() -> Vector<u64> {
    let vec = Vector::new("test_multiple_operations");
    assert!(vec.is_empty());
    vec
}

pub(crate) fn test_remove_invalid_index_prepare() -> Vector<u64> {
    let mut vec = Vector::new("test_remove_invalid_index");
    vec.push(1);
    vec
}
