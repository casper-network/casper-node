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

const GREET_RETURN_VALUE: u64 = 123456789;

#[casper(entry_points)]
trait Trait1 {
    fn abstract_greet(&self);
    fn greet(&self, who: String) -> u64 {
        log!("Hello from greet, {who}!");
        GREET_RETURN_VALUE
    }
}

#[derive(Default, Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug)]
#[casper(impl_traits(Trait1))]
struct HasTraits;

// Does not implement a manifest

#[casper(entry_points)]
impl Trait1 for HasTraits {
    fn abstract_greet(&self) {
        log!("Hello from abstract greet impl!");
    }
}

// Generates a static manifest

#[casper(entry_points)]
impl HasTraits {
    pub fn foobar(&self) {
        log!("Foobar!");
    }
}

#[cfg(test)]
mod tests {
    use crate::{HasTraits, Trait1};

    use super::Trait1Ext;
    use casper_macros::selector;
    use casper_sdk::{
        host::{
            self,
            native::{dispatch_with, Stub},
        },
        types::ResultCode,
        Contract,
    };

    #[test]
    fn foo() {
        let _ = dispatch_with(Stub::default(), || {
            let manifest = HasTraits::default_create().expect("Create");

            {
                let ret = host::call::<_, u64>(
                    &manifest.contract_address,
                    0,
                    super::Trait1_greet {
                        who: "World".to_string(),
                    },
                )
                .expect("Call");

                assert_eq!(ret.into_return_value(), super::GREET_RETURN_VALUE);
            }

            {
                let _ret = host::call::<_, ()>(
                    &manifest.contract_address,
                    0,
                    super::Trait1_abstract_greet {},
                )
                .expect("Call");

                let _ret =
                    host::call::<_, ()>(&manifest.contract_address, 0, super::HasTraits_foobar {})
                        .expect("Call");
            }
        });
    }
}
