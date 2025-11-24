#![no_std]
#![no_main]

extern crate alloc;

mod iterable_map;
mod types;
use casper_contract_sdk::prelude::*;
use iterable_map::*;

#[casper(contract_state)]
pub struct CollectionsTestContract {
    iterable_map_test_data: IterableMapTestData,
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
        }
    }

    pub(crate) fn assertions(&mut self) {
        self.iterable_map_test_data.do_assertions();
    }
}
