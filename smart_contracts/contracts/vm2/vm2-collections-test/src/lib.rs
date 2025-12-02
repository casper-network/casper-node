#![no_std]
#![no_main]

extern crate alloc;

mod iterable_map;
mod iterable_set;
mod types;
mod vector;
use casper_contract_sdk::{collections::Vector, prelude::*};
use iterable_map::*;

use crate::{iterable_set::IterableSetTestData, vector::VectorTestData};

#[casper(contract_state)]
pub struct CollectionsTestContract {
    iterable_map_test_data: IterableMapTestData,
    iterable_set_test_data: IterableSetTestData,
    vector_test_data: VectorTestData,
}

impl Default for CollectionsTestContract {
    fn default() -> Self {
        panic!("nope");
    }
}

#[casper]
impl CollectionsTestContract {
    #[casper(constructor)]
    pub fn new() -> Self {
        let iterable_map_test_data = IterableMapTestData::new();
        let iterable_set_test_data = IterableSetTestData::new();
        let vector_test_data = VectorTestData::new();
        Self {
            iterable_map_test_data,
            iterable_set_test_data,
            vector_test_data,
        }
    }

    pub(crate) fn assertions(&mut self) {
        self.iterable_map_test_data.do_assertions();
        self.iterable_set_test_data.do_assertions();
        self.vector_test_data.do_assertions();
    }

    pub(crate) fn vector_insert_out_of_bounds_panics(&mut self) {
        let mut vec = Vector::<u64>::new("vector_insert_out_of_bounds");
        vec.insert(1, 1);
    }
}
