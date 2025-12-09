use casper_contract_sdk::{collections::IterableMap, prelude::*};

use crate::types::*;

pub(crate) fn iteration_reflects_modifications_prepare() -> IterableMap<u64, String> {
    let mut map = IterableMap::<u64, String>::new("iteration_reflects_modifications");
    map.insert(1, "a".to_string());
    map.insert(2, "b".to_string());
    map
}

pub(crate) fn struct_as_key_prepare() -> IterableMap<TestKey, String> {
    let key1 = TestKey {
        id: 1,
        name: "Key1".to_string(),
    };
    let key2 = TestKey {
        id: 2,
        name: "Key2".to_string(),
    };
    let mut struct_as_key = IterableMap::<TestKey, String>::new("struct_as_key");

    struct_as_key.insert(key1.clone(), "a".to_string());
    struct_as_key.insert(key2.clone(), "b".to_string());
    struct_as_key
}

pub(crate) fn reinsert_removed_key_prepare() -> IterableMap<u64, String> {
    let mut map = IterableMap::<u64, String>::new("reinsert_removed_key");
    map.insert(1, "a".to_string());
    map.remove(&1);
    map.insert(1, "b".to_string());
    map.insert(2, "aa".to_string());
    map.remove(&2);
    map.insert(3, "aaa".to_string());
    map
}

pub(crate) fn cross_bucket_reference_prepare() -> IterableMap<CollidingKey, String> {
    let mut map = IterableMap::<CollidingKey, String>::new("cross_bucket_reference");

    // Create keys with different hashes but chained references
    let k1 = CollidingKey(1, 0);
    let k2 = CollidingKey(2, 0);
    let k3 = CollidingKey(1, 1); // Collides with k1

    map.insert(k1.clone(), "first".to_string());
    map.insert(k2.clone(), "second".to_string());
    map.insert(k3.clone(), "third".to_string());
    map
}

pub(crate) fn complex_collision_chain_prepare() -> IterableMap<CollidingKey, String> {
    let mut map = IterableMap::<CollidingKey, String>::new("complex_collision_chain");

    // Create 5 colliding keys
    let keys: Vec<_> = (0..5).map(|i| CollidingKey(42, i)).collect();

    // Insert all
    for k in &keys {
        map.insert(k.clone(), format!("{}", k.1));
    }
    map
}

pub(crate) fn full_deletion_handling_prepare() -> IterableMap<CollidingKey, String> {
    let mut map = IterableMap::<CollidingKey, String>::new("full_deletion_handling");

    let k1 = CollidingKey(42, 1);
    map.insert(k1.clone(), "lonely".to_string());
    map
}

pub(crate) fn collision_chain_iteration_prepare() -> IterableMap<CollidingKey, String> {
    let mut map = IterableMap::<CollidingKey, String>::new("collision_chain_iteration");
    let keys = [
        CollidingKey(42, 1),
        CollidingKey(42, 2),
        CollidingKey(42, 3),
    ];

    for (i, k) in keys.iter().enumerate() {
        map.insert(k.clone(), format!("value-{}", i));
    }
    map
}

pub(crate) fn tombstone_reuse_prepare() -> IterableMap<CollidingKey, String> {
    let mut map = IterableMap::<CollidingKey, String>::new("prepare_tombstone_reuse");

    let k1 = CollidingKey(42, 1);
    let k2 = CollidingKey(42, 2);

    map.insert(k1.clone(), "first".to_string());
    map.insert(k2.clone(), "second".to_string());
    map
}

pub(crate) fn tombstone_handling_prepare() -> IterableMap<CollidingKey, String> {
    let mut map = IterableMap::<CollidingKey, String>::new("prepare_tombstone_handling");

    let k1 = CollidingKey(42, 1);
    let k2 = CollidingKey(42, 2);
    let k3 = CollidingKey(42, 3);

    map.insert(k1.clone(), "first".to_string());
    map.insert(k2.clone(), "second".to_string());
    map.insert(k3.clone(), "third".to_string());
    map
}

pub(crate) fn basic_collision_handling_prepare() -> IterableMap<CollidingKey, String> {
    let mut map = IterableMap::<CollidingKey, String>::new("basic_collision_handling");

    // Both keys will have same hash but different actual keys
    let k1 = CollidingKey(42, 1);
    let k2 = CollidingKey(42, 2);

    map.insert(k1.clone(), "first".to_string());
    map.insert(k2.clone(), "second".to_string());
    map
}

pub(crate) fn unit_struct_as_key_prepare() -> IterableMap<UnitKey, String> {
    let mut map = IterableMap::<UnitKey, String>::new("unit_struct_as_key");
    map.insert(UnitKey, "value".to_string());
    map
}

pub(crate) fn multiple_removals_and_insertions_prepare() -> IterableMap<u64, String> {
    let mut map = IterableMap::<u64, String>::new("multiple_removals_and_insertions");
    map.insert(1, "a".to_string());
    map.insert(2, "b".to_string());
    map.insert(3, "c".to_string());
    map
}

pub(crate) fn remove_middle_of_long_chain_prepare() -> IterableMap<u64, String> {
    let mut map = IterableMap::<u64, String>::new("remove_middle_of_long_chain");
    map.insert(1, "a".to_string());
    map.insert(2, "b".to_string());
    map.insert(3, "c".to_string());
    map.insert(4, "d".to_string());
    map.insert(5, "e".to_string());
    map
}

pub(crate) fn remove_tail_entry_prepare() -> IterableMap<u64, String> {
    let mut m = IterableMap::<u64, String>::new("remove_tail_entry");
    m.insert(1, "a".to_string());
    m.insert(2, "b".to_string());
    m
}

pub(crate) fn skips_deleted_entries_prepare() -> IterableMap<u64, String> {
    let mut skips_deleted_entries = IterableMap::<u64, String>::new("skips_deleted_entries");
    skips_deleted_entries.insert(1, "a".to_string());
    skips_deleted_entries.insert(2, "b".to_string());
    skips_deleted_entries.insert(3, "c".to_string());
    skips_deleted_entries.remove(&2);
    skips_deleted_entries
}
pub(crate) fn remove_middle_entry_prepare() -> IterableMap<u64, String> {
    let mut remove_middle_entry = IterableMap::<u64, String>::new("remove_middle_entry");
    remove_middle_entry.insert(1, "a".to_string());
    remove_middle_entry.insert(2, "b".to_string());
    remove_middle_entry.insert(3, "c".to_string());
    remove_middle_entry
}

pub(crate) fn fetching_map_prepare() -> IterableMap<u64, String> {
    let mut iterable_map = IterableMap::<u64, String>::new("iterable_map");
    assert_eq!(iterable_map.len(), 0);
    assert_eq!(iterable_map.get(&1), None);
    iterable_map.insert(1, "a".to_string());
    assert_eq!(iterable_map.len(), 1);
    assert_eq!(iterable_map.get(&1), Some("a".to_string()));
    iterable_map.insert(4, "b".to_string());
    assert_eq!(iterable_map.len(), 2);
    assert_eq!(iterable_map.get(&4), Some("b".to_string()));
    assert_eq!(iterable_map.insert(3, "a".to_string()), None);
    assert_eq!(
        iterable_map.insert(3, "d".to_string()),
        Some("a".to_string())
    );
    assert_eq!(iterable_map.get(&3), Some("d".to_string()));
    iterable_map
}
