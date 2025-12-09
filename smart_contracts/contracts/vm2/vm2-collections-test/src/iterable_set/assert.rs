use alloc::{string::ToString, vec::Vec};
use casper_contract_sdk::{collections::IterableSet, prelude::*};

use crate::types::TestStruct;

pub(crate) fn basic_insert_contains_assert(set: &mut IterableSet<u32>) {
    assert!(set.contains(&1));
    assert!(!set.contains(&2));
    set.insert(2);
    assert!(set.contains(&2));
}

pub(crate) fn remove_elements_assert(set: &mut IterableSet<u32>) {
    assert!(!set.contains(&1));
    assert!(set.contains(&2));
    set.remove(&2);
    assert!(set.is_empty());
    assert!(!set.contains(&2));
}

pub(crate) fn iterator_order_and_contents_assert(set: &mut IterableSet<u32>) {
    let mut items: Vec<_> = set.iter().collect();
    items.sort();
    assert_eq!(items, vec![1, 2, 3]);
}

pub(crate) fn clear_functionality_assert(set: &mut IterableSet<u32>) {
    assert!(!set.is_empty());
    set.clear();
    //Second clear should do nothing
    set.clear();
    assert!(set.is_empty());
    assert_eq!(set.iter().count(), 0);
}

pub(crate) fn multiple_sets_independence_assert() {
    let mut set1 = IterableSet::new("set1");
    let mut set2 = IterableSet::new("set2");

    set1.insert(1);
    set2.insert(1);

    assert!(set1.contains(&1));
    assert!(set2.contains(&1));

    set1.remove(&1);
    assert!(!set1.contains(&1));
    assert!(set2.contains(&1));
}

pub(crate) fn struct_values_assert(set: &mut IterableSet<TestStruct>) {
    let val1 = TestStruct {
        field1: 1,
        field2: "a".to_string(),
    };
    let val2 = TestStruct {
        field1: 2,
        field2: "b".to_string(),
    };
    assert!(set.contains(&val1));
    assert!(set.contains(&val2));

    let mut collected: Vec<_> = set.iter().collect();
    collected.sort_by(|a, b| a.field1.cmp(&b.field1));
    assert_eq!(collected, vec![val1, val2]);
}

pub(crate) fn duplicate_insertions_assert(set: &mut IterableSet<u32>) {
    assert_eq!(set.iter().count(), 1);
    set.remove(&1);
    assert!(set.is_empty());
}

pub(crate) fn empty_set_behavior_assert(set: &mut IterableSet<u32>) {
    set.remove(&999); // Shouldn't panic
    assert!(set.is_empty());
}

pub(crate) fn complex_operations_sequence_assert(set: &mut IterableSet<u32>) {
    set.remove(&1);
    set.insert(3);
    set.clear();
    set.insert(4);

    let items: Vec<_> = set.iter().collect();
    assert_eq!(items, vec![4]);
}
