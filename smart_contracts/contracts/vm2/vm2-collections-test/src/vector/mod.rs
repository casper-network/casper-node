mod assert;
mod prepare;

use crate::types::*;
use alloc::vec;
use assert::*;
use casper_contract_sdk::{collections::Vector, macros::casper};
use prepare::*;

#[casper]
pub(crate) struct VectorTestData {
    should_not_panic_with_empty: Vector<u64>,
    should_retain: Vector<u64>,
    test_vec: Vector<u64>,
    test_pop: Vector<u64>,
    test_contains: Vector<u64>,
    test_clear: Vector<u64>,
    test_binary_search: Vector<u64>,
    test_swap_remove: Vector<u64>,
    test_insert_at_len: Vector<u64>,
    test_struct_elements: Vector<VectorTestStruct>,
    test_multiple_operations: Vector<u64>,
    test_remove_invalid_index: Vector<u64>,
}

impl VectorTestData {
    pub(crate) fn new() -> Self {
        Self {
            should_not_panic_with_empty: should_not_panic_with_empty_prepare(),
            should_retain: should_retain_prepare(),
            test_vec: test_vec_prepare(),
            test_pop: test_pop_prepare(),
            test_contains: test_contains_prepare(),
            test_clear: test_clear_prepare(),
            test_binary_search: test_binary_search_prepare(),
            test_swap_remove: test_swap_remove_prepare(),
            test_insert_at_len: test_insert_at_len_prepare(),
            test_struct_elements: test_struct_elements_prepare(),
            test_multiple_operations: test_multiple_operations_prepare(),
            test_remove_invalid_index: test_remove_invalid_index_prepare(),
        }
    }

    pub(crate) fn do_assertions(&mut self) {
        let should_not_panic_with_empty = &mut self.should_not_panic_with_empty;
        let should_retain = &mut self.should_retain;
        let test_vec = &mut self.test_vec;
        let test_pop = &mut self.test_pop;
        let test_contains = &mut self.test_contains;
        let test_clear = &mut self.test_clear;
        let test_binary_search = &mut self.test_binary_search;
        let test_swap_remove = &mut self.test_swap_remove;
        let test_insert_at_len = &mut self.test_insert_at_len;
        let test_struct_elements = &mut self.test_struct_elements;
        let test_multiple_operations = &mut self.test_multiple_operations;
        let test_remove_invalid_index = &mut self.test_remove_invalid_index;
        should_not_panic_with_empty_assert(should_not_panic_with_empty);
        should_retain_assert(should_retain);
        test_vec_assert(test_vec);
        test_pop_assert(test_pop);
        test_contains_assert(test_contains);
        test_clear_assert(test_clear);
        test_binary_search_assert(test_binary_search);
        test_swap_remove_assert(test_swap_remove);
        test_insert_at_len_assert(test_insert_at_len);
        test_struct_elements_assert(test_struct_elements);
        test_multiple_operations_assert(test_multiple_operations);
        test_remove_invalid_index_assert(test_remove_invalid_index);
    }
}
