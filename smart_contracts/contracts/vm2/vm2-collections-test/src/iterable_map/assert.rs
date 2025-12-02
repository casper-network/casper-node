use casper_contract_sdk::{collections::IterableMap, prelude::*, types::HashAlgorithm};
use casper_executor_wasm_common::{
    keyspace::{CollectionAddrInner, ContextAddr, Keyspace},
    CollectionTypeTag,
};

use crate::types::{CollidingKey, TestKey, UnitKey};

pub(crate) fn remove_tail_entry_assert(remove_tail_entry: &mut IterableMap<u64, String>) {
    assert_eq!(remove_tail_entry.remove(&2), Some("b".to_string()));
    assert_eq!(remove_tail_entry.len(), 1);
    let values: Vec<_> = remove_tail_entry.values().collect();
    assert_eq!(values, vec!["a".to_string(),]);
}

pub(crate) fn fetching_test_assert(fetching_test: &mut IterableMap<u64, String>) {
    assert_eq!(fetching_test.len(), 3);
    assert_eq!(fetching_test.get(&1), Some("a".to_string()));
    assert_eq!(fetching_test.get(&4), Some("b".to_string()));
    assert_eq!(fetching_test.get(&3), Some("d".to_string()));
    //remove_nonexistent_key_does_nothing
    assert_eq!(fetching_test.remove(&999), None);
    assert_eq!(fetching_test.get(&1), Some("a".to_string()));
    //keys_returns_reverse_insertion_order
    let hashes: Vec<_> = fetching_test.keys().collect();
    assert_eq!(hashes, vec![3, 4, 1]);

    //iterates_all_entries_in_reverse_insertion_order
    let values: Vec<_> = fetching_test.values().collect();
    assert_eq!(
        values,
        vec!["d".to_string(), "b".to_string(), "a".to_string(),]
    );
}

pub(crate) fn remove_middle_entry_assert(remove_middle_entry: &mut IterableMap<u64, String>) {
    assert_eq!(remove_middle_entry.remove(&2), Some("b".to_string()));
    //#TODO REMOVE DIDN'T REMOVE!
    //assert_eq!(remove_middle_entry.get(&2), None);
    //assert!(!remove_middle_entry.contains_key(&2));
    assert_eq!(remove_middle_entry.len(), 2);
    //#TODO Second remove panics!!
    //assert_eq!(remove_middle_entry.remove(&2), None);
    assert_eq!(remove_middle_entry.get(&1), Some("a".to_string()));
    assert_eq!(remove_middle_entry.get(&3), Some("c".to_string()));
}

pub(crate) fn separate_maps_do_not_conflict_assert() {
    let map1 = IterableMap::<u64, String>::new("map1");
    let map2 = IterableMap::<u64, String>::new("map2");
    assert_eq!(map1.get(&1), Some("a".to_string()));
    assert_eq!(map2.get(&1), Some("b".to_string()));
}

pub(crate) fn clear_removes_all_entries_assert(to_clear: &mut IterableMap<u64, String>) {
    assert_eq!(to_clear.iter().count(), 2);
    to_clear.clear();
    assert!(to_clear.is_empty());
    assert_eq!(to_clear.iter().count(), 0);
    //#TODO CLEAR DIDNT REMOVE!
    //assert_eq!(to_clear.get(&1), None);
}

pub(crate) fn insert_after_remove_updates_head_assert(
    insert_after_remove_updates_head: &mut IterableMap<u64, String>,
) {
    insert_after_remove_updates_head.remove(&2);
    insert_after_remove_updates_head.insert(3, "c".to_string());
    let values: Vec<_> = insert_after_remove_updates_head.values().collect();
    assert_eq!(values, vec!["c", "a"]);
}

pub(crate) fn struct_as_key_assert(struct_as_key: &IterableMap<TestKey, String>) {
    //struct_as_key
    let key1 = TestKey {
        id: 1,
        name: "Key1".to_string(),
    };
    let key2 = TestKey {
        id: 2,
        name: "Key2".to_string(),
    };
    assert_eq!(struct_as_key.get(&key1), Some("a".to_string()));
    assert_eq!(struct_as_key.get(&key2), Some("b".to_string()));
}

pub(crate) fn full_deletion_handling_assert(map: &mut IterableMap<CollidingKey, String>) {
    let k1 = CollidingKey(42, 1);
    assert_eq!(map.remove(&k1), Some("lonely".to_string()));

    // Verify complete removal
    let (_, entry) = map.get_writable_slot(&k1);
    assert!(entry.is_none());
}

pub(crate) fn reinsert_removed_key_assert(reinsert_removed_key: &mut IterableMap<u64, String>) {
    assert_eq!(reinsert_removed_key.get(&1), Some("b".to_string()));
    reinsert_removed_key.remove(&2);
    reinsert_removed_key.insert(2, "bb".to_string());
    reinsert_removed_key.remove(&3);
    reinsert_removed_key.insert(3, "bbb".to_string());
    assert_eq!(reinsert_removed_key.get(&3), Some("bbb".to_string()));
    let keys_and_values: Vec<(u64, String)> = reinsert_removed_key.iter().collect();
    assert_eq!(
        keys_and_values,
        vec![(3, "bbb".into()), (2, "bb".into()), (1, "b".into())]
    )
}

pub(crate) fn basic_collision_handling_assert(map: &IterableMap<CollidingKey, String>) {
    let k1 = CollidingKey(42, 1);
    let k2 = CollidingKey(42, 2);
    assert_eq!(map.get(&k1), Some("first".to_string()));
    assert_eq!(map.get(&k2), Some("second".to_string()));
}

pub(crate) fn tombstone_handling_assert(map: &mut IterableMap<CollidingKey, String>) {
    let k2 = CollidingKey(42, 2);

    // Remove middle entry
    assert_eq!(map.remove(&k2), Some("second".to_string()));

    // Verify tombstone state
    let (_, entry) = map.get_writable_slot(&k2);
    assert!(entry.unwrap().get_value().is_none());

    // Verify chain integrity
    let values: Vec<_> = map.values().collect();
    assert_eq!(values, vec!["third", "first"]);
}

pub(crate) fn tombstone_reuse_assert(map: &mut IterableMap<CollidingKey, String>) {
    let k1 = CollidingKey(42, 1);
    let k2 = CollidingKey(42, 2);
    // Removing k1 while k2 exists guarantees k1 turns into
    // a tombstone
    map.remove(&k1);

    // Reinsert into tombstone slot
    map.insert(k1.clone(), "reused".to_string());

    assert_eq!(map.get(&k1), Some("reused".to_string()));
    assert_eq!(map.get(&k2), Some("second".to_string()));
}

pub(crate) fn cross_bucket_reference_assert(map: &mut IterableMap<CollidingKey, String>) {
    let k2 = CollidingKey(2, 0);
    // Remove k2 which is referenced by k3
    map.remove(&k2);

    // Verify iteration skips removed entry
    let values: Vec<_> = map.values().collect();
    assert_eq!(values, vec!["third", "first"]);
}

pub(crate) fn complex_collision_chain_assert(map: &mut IterableMap<CollidingKey, String>) {
    // Create 5 colliding keys
    let keys: Vec<_> = (0..5).map(|i| CollidingKey(42, i)).collect();
    // Remove even indexes
    for k in keys.iter().step_by(2) {
        map.remove(k);
    }

    // Insert new values
    map.insert(keys[0].clone(), "reinserted".to_string());
    map.insert(CollidingKey(42, 5), "new".to_string());

    // Verify final state
    let expected = vec![
        ("new".to_string(), 5),
        ("reinserted".to_string(), 0),
        ("3".to_string(), 3),
        ("1".to_string(), 1),
    ];

    let results: Vec<_> = map.iter().map(|(k, v)| (v, k.1)).collect();

    assert_eq!(results, expected);
}
pub(crate) fn collision_chain_iteration_assert(map: &mut IterableMap<CollidingKey, String>) {
    // Remove middle entry
    map.remove(&CollidingKey(42, 2));

    let values: Vec<_> = map.values().collect();
    assert_eq!(values, vec!["value-2", "value-0"]);
}

pub(crate) fn multiple_removals_and_insertions_assert(map: &mut IterableMap<u64, String>) {
    map.remove(&2);
    assert_eq!(map.get(&2), None);
    assert_eq!(map.get(&1), Some("a".to_string()));
    assert_eq!(map.get(&3), Some("c".to_string()));

    map.insert(4, "d".to_string());
    let values: Vec<_> = map.values().collect();
    assert_eq!(values, vec!["d", "c", "a"]);
}

pub(crate) fn remove_middle_of_long_chain_assert(map: &mut IterableMap<u64, String>) {
    // The order is 5,4,3,2,1
    map.remove(&3); // Remove the middle entry

    let values: Vec<_> = map.values().collect();
    assert_eq!(values, vec!["e", "d", "b", "a"]);
    // Check that entry 4's previous is now 2's hash
    let ptr4 = map.create_root_ptr_from_key(&4u64);
    let prefix = map.create_prefix_from_ptr(&ptr4);
    let tail = casper::generic_hash(&prefix, HashAlgorithm::Blake2b).unwrap();
    let keyspace = Keyspace::Context(ContextAddr::from(CollectionAddrInner::new(
        *casper::get_callee().address(),
        CollectionTypeTag::IterableMap,
        [0u8; 8],
        tail,
    )));
    let entry = map.get_entry(keyspace).unwrap();
    assert_eq!(
        entry.get_previous().clone(),
        Some(map.create_root_ptr_from_key(&2u64))
    );
}

pub(crate) fn unit_struct_as_key_assert(map: &IterableMap<UnitKey, String>) {
    assert_eq!(map.get(&UnitKey), Some("value".to_string()));
}

pub(crate) fn iteration_reflects_modifications_assert(
    iteration_reflects_modifications: &mut IterableMap<u64, String>,
) {
    let mut iter = iteration_reflects_modifications.iter();
    assert_eq!(iter.next().unwrap().1, "b".to_string());

    iteration_reflects_modifications.remove(&2);
    iteration_reflects_modifications.insert(3, "c".to_string());
    let values: Vec<_> = iteration_reflects_modifications.values().collect();
    assert_eq!(values, vec!["c", "a"]);
}
