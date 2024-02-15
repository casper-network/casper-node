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

#[casper(trait_definition)]
trait Trait1 {
    fn abstract_greet(&self);

    fn greet(&self, who: String) -> u64 {
        log!("Hello from greet, {who}!");
        GREET_RETURN_VALUE
    }

    fn adder(lhs: u64, rhs: u64) -> u64;
}

#[derive(Default, BorshSerialize, BorshDeserialize, CasperABI, Debug, Copy, Clone)]
struct CounterState {
    value: u64,
}

#[casper(trait_definition)]
trait Counter {
    fn increment(&mut self) {
        log!("Incrementing!");
        self.counter_state_mut().value += 1;
    }

    fn decrement(&mut self) {
        log!("Decrementing!");
        self.counter_state_mut().value -= 1;
    }

    fn get_counter_value(&self) -> u64 {
        self.counter_state().value
    }

    #[casper(private)]
    fn counter_state(&self) -> &CounterState;

    #[casper(private)]
    fn counter_state_mut(&mut self) -> &mut CounterState;
}

#[derive(Default, Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug)]
#[casper(impl_traits(Trait1, Counter))]
struct HasTraits {
    counter_state: CounterState,
}

impl Trait1 for HasTraits {
    fn abstract_greet(&self) {
        log!("Hello from abstract greet impl!");
    }

    fn adder(lhs: u64, rhs: u64) -> u64 {
        lhs + rhs
    }
}

// Implementing traits does not require extra annotation as the trait dispatcher is generated at the
// trait level.
impl Counter for HasTraits {
    fn counter_state_mut(&mut self) -> &mut CounterState {
        &mut self.counter_state
    }
    fn counter_state(&self) -> &CounterState {
        &self.counter_state
    }
}

#[casper(contract)]
impl HasTraits {
    pub fn foobar(&self) {
        // Can extend contract that implements a trait to also call methods provided by a trait.
        let counter_state = self.counter_state();
        log!("Foobar! Counter value: {}", counter_state.value);
    }
}

// struct HasTraitsRef { foobar() }

#[cfg(test)]
mod tests {
    use crate::{CounterState, HasTraits, Trait1};

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

    #[should_panic(query = "Entry point exists")]
    #[test]
    fn cant_call_private() {
        let _ = dispatch_with(Stub::default(), || {
            let manifest = HasTraits::default_create().expect("Create");

            // TODO: native impl currently is panicking, fix error handling in it
            {
                let _ret = host::casper_call(
                    &manifest.contract_address,
                    0,
                    selector!("get_counter_state"),
                    &[],
                );
            }
        });
    }
    #[test]
    fn foo() {
        let _ = dispatch_with(Stub::default(), || {
            let manifest = HasTraits::default_create().expect("Create");

            // <HasTraits as Trait1>::greet();

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

            {
                let ret = host::call::<_, u64>(
                    &manifest.contract_address,
                    0,
                    super::Trait1_adder { lhs: 1, rhs: 2 },
                )
                .expect("Call");

                assert_eq!(ret.into_return_value(), 3);
            }

            //
            // Counter trait
            //

            {
                let ret = host::call::<_, u64>(
                    &manifest.contract_address,
                    0,
                    super::Counter_get_counter_value {},
                )
                .expect("Call");

                assert_eq!(ret.into_return_value(), 0);

                let _ret =
                    host::call::<_, ()>(&manifest.contract_address, 0, super::Counter_increment {})
                        .expect("Call");

                let ret = host::call::<_, u64>(
                    &manifest.contract_address,
                    0,
                    super::Counter_get_counter_value {},
                )
                .expect("Call");

                assert_eq!(ret.into_return_value(), 1);
            }
        });
    }
}
