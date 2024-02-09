#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

#[macro_use]
extern crate alloc;

use core::marker::PhantomData;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use borsh::{BorshDeserialize, BorshSerialize};
use casper_macros::{casper, selector, CasperABI, CasperSchema, Contract};
use casper_sdk::{
    host::{self, Alloc, CallResult},
    log, revert,
    sys::CreateResult,
    types::{Address, CallError, ResultCode},
    Contract, Selector, ToCallData,
};

trait Trait1 {
    fn trait1_functionality() {}
}

struct Trait1Ext;

// struct Trait1Ext;

// struct Trait1Ext {
//     fn manifest() -> Manifest;
//     fn schema() -> Schema;
// }

#[derive(Default, Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug)]
struct Traits;

#[casper(entry_points)]
impl Trait1 for Traits {}

#[casper(entry_points)]
impl Traits {
    pub fn foo() {}
}

#[cfg(test)]
mod tests {
    use crate::Traits;
    use casper_sdk::schema::CasperSchema;

    #[test]
    fn foo() {
        let schema = Traits::schema();
        assert!(schema.entry_points.iter().any(|e| e.name == "foo"));
        assert!(schema
            .entry_points
            .iter()
            .any(|e| e.name == "trait1_functionality"));
    }
}
