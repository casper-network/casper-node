mod assert;
mod prepare;

use crate::types::*;
use assert::*;
use casper_contract_sdk::{collections::IterableSet, macros::casper};
use prepare::*;

#[casper]
pub(crate) struct IterableSetTestData {
    basic_insert_contains: IterableSet<u32>,
    remove_elements: IterableSet<u32>,
    iterator_order_and_contents: IterableSet<u32>,
    clear_functionality: IterableSet<u32>,
    struct_values: IterableSet<TestStruct>,
    duplicate_insertions: IterableSet<u32>,
    empty_set_behavior: IterableSet<u32>,
    complex_operations_sequence: IterableSet<u32>,
}

impl IterableSetTestData {
    pub(crate) fn new() -> Self {
        Self {
            basic_insert_contains: basic_insert_contains_prepare(),
            remove_elements: remove_elements_prepare(),
            iterator_order_and_contents: iterator_order_and_contents_prepare(),
            clear_functionality: clear_functionality_prepare(),
            struct_values: struct_values_prepare(),
            duplicate_insertions: duplicate_insertions_prepare(),
            empty_set_behavior: empty_set_behavior_prepare(),
            complex_operations_sequence: complex_operations_sequence_prepare(),
        }
    }

    pub(crate) fn do_assertions(&mut self) {
        let basic_insert_contains = &mut self.basic_insert_contains;
        let remove_elements = &mut self.remove_elements;
        let iterator_order_and_contents = &mut self.iterator_order_and_contents;
        let clear_functionality = &mut self.clear_functionality;
        let struct_values = &mut self.struct_values;
        let duplicate_insertions = &mut self.duplicate_insertions;
        let empty_set_behavior = &mut self.empty_set_behavior;
        let complex_operations_sequence = &mut self.complex_operations_sequence;
        basic_insert_contains_assert(basic_insert_contains);
        remove_elements_assert(remove_elements);
        iterator_order_and_contents_assert(iterator_order_and_contents);
        clear_functionality_assert(clear_functionality);
        multiple_sets_independence_assert();
        struct_values_assert(struct_values);
        duplicate_insertions_assert(duplicate_insertions);
        empty_set_behavior_assert(empty_set_behavior);
        complex_operations_sequence_assert(complex_operations_sequence);
    }
}
