use alloc::string::ToString;
use casper_contract_sdk::collections::IterableSet;

use crate::types::*;

pub(crate) fn basic_insert_contains_prepare() -> IterableSet<u32> {
    let mut set = IterableSet::new("test_set");
    assert!(!set.contains(&1));
    set.insert(1);
    assert!(set.contains(&1));
    set
}

pub(crate) fn remove_elements_prepare() -> IterableSet<u32> {
    let mut set = IterableSet::new("remove_elements");
    set.insert(1);
    set.insert(2);
    set.remove(&1);
    assert!(!set.contains(&1));
    assert!(set.contains(&2));
    // Removing non-existing value should do nothing
    set.remove(&1);
    set
}

pub(crate) fn iterator_order_and_contents_prepare() -> IterableSet<u32> {
    let mut set = IterableSet::new("iterator_order_and_contents");
    set.insert(1);
    set.insert(2);
    set.insert(3);
    set
}

pub(crate) fn clear_functionality_prepare() -> IterableSet<u32> {
    let mut set = IterableSet::new("clear_functionality");
    set.insert(1);
    set.insert(2);

    assert!(!set.is_empty());
    set
}

pub(crate) fn struct_values_prepare() -> IterableSet<TestStruct> {
    let val1 = TestStruct {
        field1: 1,
        field2: "a".to_string(),
    };
    let val2 = TestStruct {
        field1: 2,
        field2: "b".to_string(),
    };

    let mut set = IterableSet::new("struct_values");
    set.insert(val1.clone());
    set.insert(val2.clone());
    set
}

pub(crate) fn duplicate_insertions_prepare() -> IterableSet<u32> {
    let mut set = IterableSet::new("duplicate_insertions");
    set.insert(1);
    set.insert(1); // Should be no-op
    assert_eq!(set.iter().count(), 1);
    set
}

pub(crate) fn empty_set_behavior_prepare() -> IterableSet<u32> {
    let set = IterableSet::<u32>::new("empty_set_behavior");
    assert!(set.is_empty());
    assert_eq!(set.iter().count(), 0);

    let mut set = set;
    set.remove(&999); // Shouldn't panic
    assert!(set.is_empty());
    set
}

pub(crate) fn complex_operations_sequence_prepare() -> IterableSet<u32> {
    let mut set = IterableSet::new("complex_operations_sequence");
    set.insert(1);
    set.insert(2);
    set
}
