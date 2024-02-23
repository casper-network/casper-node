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
    Contract, ContractHandle, Selector, ToCallData,
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

#[derive(Default, BorshSerialize, BorshDeserialize, CasperABI, Debug, Copy, Clone, PartialEq)]
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

    fn get_counter_state(&self) -> CounterState {
        self.counter_state().clone()
    }

    #[casper(private)]
    fn counter_state(&self) -> &CounterState;

    #[casper(private)]
    fn counter_state_mut(&mut self) -> &mut CounterState;
}

#[derive(
    Default, Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug, Clone,
)]
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
    #[casper(constructor)]
    pub fn new(counter_value: u64) -> Self {
        log!("Calling new constructor {counter_value}");
        Self {
            counter_state: CounterState {
                value: counter_value,
            },
        }
    }
    pub fn foobar(&self) {
        // Can extend contract that implements a trait to also call methods provided by a trait.
        let counter_state = self.counter_state();
        log!("Foobar! Counter value: {}", counter_state.value);
    }
}

pub fn perform_test() {
    let contract_handle = HasTraits::default_create().expect("Create");
    let trait1_handle =
        ContractHandle::<Trait1Ref>::from_address(contract_handle.contract_address());
    let counter_handle =
        ContractHandle::<CounterRef>::from_address(contract_handle.contract_address());

    {
        let greet_result: u64 = contract_handle
            .build_call()
            .cast::<Trait1Ref>()
            .call(|trait1ref| trait1ref.greet("World".into()))
            .expect("Call as Trait1Ref");
        assert_eq!(greet_result, GREET_RETURN_VALUE);
    }

    {
        let () = trait1_handle
            .call(|trait1ref| trait1ref.abstract_greet())
            .expect("Call as Trait1Ref");
    }

    {
        let result: u64 = contract_handle
            .build_call()
            .cast::<Trait1Ref>()
            .call(|trait1ref| trait1ref.adder(1111, 2222))
            .expect("Call as Trait1Ref");
        assert_eq!(result, 1111 + 2222);
    }

    //
    // Counter trait
    //

    {
        let counter_value = counter_handle
            .call(|counter| counter.get_counter_value())
            .expect("Call");
        assert_eq!(counter_value, 0);

        // call increase
        let () = counter_handle
            .call(|counter| counter.increment())
            .expect("Call");

        // get value
        let counter_value = counter_handle
            .call(|counter| counter.get_counter_value())
            .expect("Call");

        // check that the value increased
        assert_eq!(counter_value, 1);

        // call decrease
        let () = counter_handle
            .call(|counter| counter.decrement())
            .expect("Call");

        // get value and compare the difference
        let counter_value = counter_handle
            .call(|counter| counter.get_counter_value())
            .expect("Call");
        assert_eq!(counter_value, 0);
    }
}

#[cfg(test)]
mod tests {
    use crate::{CounterRef, CounterState, HasTraits, Trait1, GREET_RETURN_VALUE};

    use super::Trait1Ref;
    use alloc::collections::BTreeSet;
    use casper_macros::selector;
    use casper_sdk::{
        abi::{Definition, StructField},
        host::{
            self,
            native::{dispatch_with, Environment},
        },
        log,
        schema::{CasperSchema, SchemaEntryPoint, SchemaType},
        sys::CreateResult,
        Contract, ContractHandle, ContractRef,
    };
    use vm_common::flags::EntryPointFlags;

    #[should_panic(query = "Entry point exists")]
    #[test]
    fn cant_call_private1() {
        let _ = dispatch_with(Environment::default(), || {
            let has_traits_handle = HasTraits::default_create().expect("Create");

            // TODO: native impl currently is panicking, fix error handling in it
            {
                let _ret = host::casper_call(
                    &has_traits_handle.contract_address(),
                    0,
                    selector!("counter_state"),
                    &[],
                );
            }
        });
    }

    #[should_panic(query = "Entry point exists")]
    #[test]
    fn cant_call_private2() {
        let _ = dispatch_with(Environment::default(), || {
            let has_traits_handle = HasTraits::default_create().expect("Create");

            // TODO: native impl currently is panicking, fix error handling in it
            {
                let _ret = host::casper_call(
                    &has_traits_handle.contract_address(),
                    0,
                    selector!("counter_state_mut"),
                    &[],
                );
            }
        });
    }

    use super::Counter;
    use casper_sdk::abi::CasperABI;
    #[test]
    fn unit_test() {
        let mut has_traits = HasTraits::default();
        has_traits.increment();
    }

    #[test]
    fn trait_has_schema() {
        // We can't attach methods to trait itself, but we can generate an "${TRAIT}Ext" struct and
        // attach extra information to it. let schema = Trait1::schema();
        let counter_schema = super::CounterRef::schema();

        assert_eq!(counter_schema.type_, SchemaType::Interface);

        // Order of entry point definitions is not guaranteed.
        assert_eq!(
            BTreeSet::from_iter(counter_schema.entry_points.clone()),
            BTreeSet::from_iter([
                SchemaEntryPoint {
                    name: "get_counter_value".to_string(),
                    selector: selector!("get_counter_value").get(),
                    arguments: vec![],
                    result: "U64".to_string(),
                    flags: EntryPointFlags::empty()
                },
                SchemaEntryPoint {
                    name: "get_counter_state".to_string(),
                    selector: selector!("get_counter_state").get(),
                    arguments: vec![],
                    result: "vm2_trait::CounterState".to_string(),
                    flags: EntryPointFlags::empty()
                },
                SchemaEntryPoint {
                    name: "decrement".to_string(),
                    selector: selector!("decrement").get(),
                    arguments: vec![],
                    result: "()".to_string(),
                    flags: EntryPointFlags::empty()
                },
                SchemaEntryPoint {
                    name: "increment".to_string(),
                    selector: selector!("increment").get(),
                    arguments: vec![],
                    result: "()".to_string(),
                    flags: EntryPointFlags::empty()
                },
            ])
        );
    }

    #[test]
    fn schema_has_traits() {
        let schema = HasTraits::schema();

        assert_eq!(
            schema.type_,
            SchemaType::Contract {
                state: "vm2_trait::HasTraits".to_string()
            }
        );

        assert!(
            schema.entry_points.iter().any(|e| e.name == "foobar"),
            "Method inside impl block"
        );

        assert!(
            schema.entry_points.iter().any(|e| e.name == "increment"),
            "Method inside Counter trait"
        );

        let get_counter_state = schema
            .entry_points
            .iter()
            .find(|e| e.name == "get_counter_state")
            .unwrap();
        let counter_state_def = schema
            .definitions
            .get(&get_counter_state.result)
            .expect("Has counter state definition");

        let expected_definition = vec![StructField {
            name: "value".to_string(),
            decl: <u64>::declaration(),
        }];
        assert_eq!(
            counter_state_def
                .as_struct()
                .expect("Counter State is struct"),
            expected_definition.as_slice()
        );

        assert!(
            !schema
                .entry_points
                .iter()
                .any(|e| e.name == "counter_state"),
            "Trait method marked as private"
        );
        assert!(
            !schema
                .entry_points
                .iter()
                .any(|e| e.name == "counter_state_mut"),
            "Trait method marked as private"
        );
    }

    #[test]
    fn foo_with_custom_constructor() {
        let _ret = dispatch_with(Environment::default(), || {
            let constructor = super::HasTraitsRef::new(5);

            let has_traits_handle = HasTraits::create(constructor).expect("Constructor works");

            let value = host::call(
                &has_traits_handle.contract_address(),
                0,
                super::CounterRef::new().get_counter_value(),
            )
            .expect("Call");

            assert_eq!(value.into_return_value(), 5);
        });
        log!("OK");
    }

    #[test]
    fn foo() {
        let _ = dispatch_with(Environment::default(), || {
            super::perform_test();
        });

        log!("Success");
    }
}
