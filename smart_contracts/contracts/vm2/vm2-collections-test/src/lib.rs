#![no_std]
#![no_main]

extern crate alloc;

mod iterable_map;
mod iterable_set;
mod types;
use casper_contract_sdk::prelude::*;
use iterable_map::*;

use crate::iterable_set::IterableSetTestData;

#[casper(contract_state)]
pub struct CollectionsTestContract {
    iterable_map_test_data: IterableMapTestData,
    iterable_set_test_data: IterableSetTestData,
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
        Self {
            iterable_map_test_data: IterableMapTestData::new(),
            iterable_set_test_data: IterableSetTestData::new(),
        }
    }

    pub(crate) fn assertions(&mut self) {
        //self.iterable_map_test_data.do_assertions();
        self.iterable_set_test_data.do_assertions();
    }
}
