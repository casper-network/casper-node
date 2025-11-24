mod assert;
mod prepare;

use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};
use assert::*;
use casper_contract_sdk::{collections::IterableMap, macros::casper};
use prepare::*;

use crate::types::{CollidingKey, TestKey, UnitKey};

#[casper]
pub(crate) struct IterableMapTestData {
    fetching_test: IterableMap<u64, String>,
    remove_tail_entry: IterableMap<u64, String>,
    remove_middle_entry: IterableMap<u64, String>,
    skips_deleted_entries: IterableMap<u64, String>,
    shared: IterableMap<u64, String>,
    to_clear: IterableMap<u64, String>,
    struct_as_key: IterableMap<TestKey, String>,
    insert_after_remove_updates_head: IterableMap<u64, String>,
    reinsert_removed_key: IterableMap<u64, String>,
    iteration_reflects_modifications: IterableMap<u64, String>,
    multiple_removals_and_insertions: IterableMap<u64, String>,
    remove_middle_of_long_chain: IterableMap<u64, String>,
    unit_struct_as_key: IterableMap<UnitKey, String>,
    basic_collision_handling: IterableMap<CollidingKey, String>,
    tombstone_handling: IterableMap<CollidingKey, String>,
    tombstone_reuse: IterableMap<CollidingKey, String>,
    collision_chain_iteration: IterableMap<CollidingKey, String>,
    complex_collision_chain: IterableMap<CollidingKey, String>,
    cross_bucket_reference: IterableMap<CollidingKey, String>,
    full_deletion_handling: IterableMap<CollidingKey, String>,
}

impl IterableMapTestData {
    pub(crate) fn new() -> Self {
        // empty_map_behaves_sanely
        let mut map = IterableMap::<u64, String>::new("map");
        map.remove(&3);
        assert_eq!(map.get(&1), None);
        assert_eq!(map.remove(&1), None);
        assert_eq!(map.iter().count(), 0);
        let fetching_test = fetching_map_prepare();
        let remove_tail_entry = remove_tail_entry_prepare();
        let remove_middle_entry = remove_middle_entry_prepare();
        let skips_deleted_entries = skips_deleted_entries_prepare();

        // separate_maps_do_not_conflict
        let mut map1 = IterableMap::<u64, String>::new("map1");
        let mut map2 = IterableMap::<u64, String>::new("map2");
        map1.insert(1, "a".to_string());
        map2.insert(1, "b".to_string());

        // insert_same_value_under_different_keys
        let mut shared = IterableMap::<u64, String>::new("shared");
        shared.insert(1, "shared".to_string());
        shared.insert(2, "shared".to_string());

        // clear_removes_all_entries
        let mut to_clear = IterableMap::<u64, String>::new("to_clear");
        to_clear.insert(1, "a".to_string());
        to_clear.insert(2, "b".to_string());

        // struct_as_key
        let struct_as_key = struct_as_key_prepare();

        //insert_after_remove_updates_head
        let mut insert_after_remove_updates_head =
            IterableMap::<u64, String>::new("insert_after_remove_updates_head");
        insert_after_remove_updates_head.insert(1, "a".to_string());
        insert_after_remove_updates_head.insert(2, "b".to_string());

        let iteration_reflects_modifications = iteration_reflects_modifications_prepare();
        let reinsert_removed_key = reinsert_removed_key_prepare();
        let unit_struct_as_key = unit_struct_as_key_prepare();
        let basic_collision_handling = basic_collision_handling_prepare();
        let tombstone_handling = tombstone_handling_prepare();
        let tombstone_reuse = tombstone_reuse_prepare();
        let collision_chain_iteration = collision_chain_iteration_prepare();
        let complex_collision_chain = complex_collision_chain_prepare();
        let cross_bucket_reference = cross_bucket_reference_prepare();
        let full_deletion_handling = full_deletion_handling_prepare();
        let multiple_removals_and_insertions = multiple_removals_and_insertions_prepare();
        let remove_middle_of_long_chain = remove_middle_of_long_chain_prepare();

        Self {
            fetching_test,
            remove_tail_entry,
            remove_middle_entry,
            skips_deleted_entries,
            shared,
            to_clear,
            struct_as_key,
            insert_after_remove_updates_head,
            reinsert_removed_key,
            iteration_reflects_modifications,
            unit_struct_as_key,
            basic_collision_handling,
            tombstone_handling,
            tombstone_reuse,
            collision_chain_iteration,
            complex_collision_chain,
            cross_bucket_reference,
            full_deletion_handling,
            multiple_removals_and_insertions,
            remove_middle_of_long_chain,
        }
    }

    pub(crate) fn do_assertions(&mut self) {
        let remove_tail_entry = &mut self.remove_tail_entry;
        let remove_middle_entry = &mut self.remove_middle_entry;
        let fetching_test = &mut self.fetching_test;
        let skips_deleted_entries = &mut self.skips_deleted_entries;
        let struct_as_key = &self.struct_as_key;
        let shared = &self.shared;
        let to_clear = &mut self.to_clear;
        let insert_after_remove_updates_head = &mut self.insert_after_remove_updates_head;
        let reinsert_removed_key = &mut self.reinsert_removed_key;
        let iteration_reflects_modifications = &mut self.iteration_reflects_modifications;
        let unit_struct_as_key = &self.unit_struct_as_key;
        let basic_collision_handling = &self.basic_collision_handling;
        let tombstone_handling = &mut self.tombstone_handling;
        let tombstone_reuse = &mut self.tombstone_reuse;
        let collision_chain_iteration = &mut self.collision_chain_iteration;
        let complex_collision_chain = &mut self.complex_collision_chain;
        let cross_bucket_reference = &mut self.cross_bucket_reference;
        let multiple_removals_and_insertions = &mut self.multiple_removals_and_insertions;
        let remove_middle_of_long_chain = &mut self.remove_middle_of_long_chain;
        let full_deletion_handling = &mut self.full_deletion_handling;

        let values: Vec<_> = skips_deleted_entries.values().collect();
        assert_eq!(values, vec!["c".to_string(), "a".to_string(),]);
        remove_tail_entry_assert(remove_tail_entry);
        fetching_test_assert(fetching_test);
        separate_maps_do_not_conflict_assert();
        // insert_same_value_under_different_keys
        assert_eq!(shared.get(&1), Some("shared".to_string()));
        assert_eq!(shared.get(&2), Some("shared".to_string()));

        remove_middle_entry_assert(remove_middle_entry);
        clear_removes_all_entries_assert(to_clear);
        struct_as_key_assert(struct_as_key);
        insert_after_remove_updates_head_assert(insert_after_remove_updates_head);
        reinsert_removed_key_assert(reinsert_removed_key);
        iteration_reflects_modifications_assert(iteration_reflects_modifications);
        unit_struct_as_key_assert(unit_struct_as_key);
        basic_collision_handling_assert(basic_collision_handling);
        tombstone_handling_assert(tombstone_handling);
        tombstone_reuse_assert(tombstone_reuse);
        collision_chain_iteration_assert(collision_chain_iteration);
        complex_collision_chain_assert(complex_collision_chain);
        cross_bucket_reference_assert(cross_bucket_reference);
        multiple_removals_and_insertions_assert(multiple_removals_and_insertions);
        remove_middle_of_long_chain_assert(remove_middle_of_long_chain);
        full_deletion_handling_assert(full_deletion_handling);
    }
}
