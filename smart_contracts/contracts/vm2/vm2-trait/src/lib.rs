#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_contract_macros::blake2b256;
use casper_contract_sdk::{log, prelude::*, ContractBuilder, ContractHandle};
use casper_contract_sdk_contrib::{
    access_control::{AccessControl, AccessControlExt, AccessControlState, Role},
    ownable::{Ownable, OwnableError, OwnableExt, OwnableState},
};

pub const GREET_RETURN_VALUE: u64 = 123456789;

#[casper]
pub trait Trait1 {
    fn abstract_greet(&self);

    fn greet(&self, who: String) -> u64 {
        log!("Hello from greet, {who}!");
        GREET_RETURN_VALUE
    }

    fn adder(&self, lhs: u64, rhs: u64) -> u64;
}

#[casper]
#[derive(Copy, Clone, Default)]
pub struct CounterState {
    value: u64,
}

#[casper]
pub trait Counter {
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

#[casper(contract_state)]
#[derive(Default)]
pub struct HasTraits {
    counter_state: CounterState,
    ownable_state: OwnableState,
    access_control_state: AccessControlState,
}

#[casper]
impl Trait1 for HasTraits {
    fn abstract_greet(&self) {
        log!("Hello from abstract greet impl!");
    }

    fn adder(&self, lhs: u64, rhs: u64) -> u64 {
        lhs + rhs
    }
}

// Implementing traits does not require extra annotation as the trait dispatcher is generated at the
// trait level.
#[casper]
impl Counter for HasTraits {
    fn counter_state_mut(&mut self) -> &mut CounterState {
        &mut self.counter_state
    }
    fn counter_state(&self) -> &CounterState {
        &self.counter_state
    }
}

#[casper(path = casper_contract_sdk_contrib::ownable)]
impl Ownable for HasTraits {
    fn state(&self) -> &OwnableState {
        &self.ownable_state
    }
    fn state_mut(&mut self) -> &mut OwnableState {
        &mut self.ownable_state
    }
}

#[casper]
pub enum UserRole {
    Admin,
    User,
}

impl Into<Role> for UserRole {
    fn into(self) -> Role {
        match self {
            UserRole::Admin => blake2b256!("admin"),
            UserRole::User => blake2b256!("user"),
        }
    }
}

#[casper(path = casper_contract_sdk_contrib::access_control)]
impl AccessControl for HasTraits {
    fn state(&self) -> &AccessControlState {
        &self.access_control_state
    }
    fn state_mut(&mut self) -> &mut AccessControlState {
        &mut self.access_control_state
    }
}

#[casper]
impl HasTraits {
    #[casper(constructor)]
    pub fn new(counter_value: u64) -> Self {
        log!("Calling new constructor with value={counter_value}");
        Self {
            counter_state: CounterState {
                value: counter_value,
            },
            ownable_state: OwnableState::default(),
            access_control_state: AccessControlState::default(),
        }
    }
    pub fn foobar(&self) {
        // Can extend contract that implements a trait to also call methods provided by a trait.
        let counter_state = self.counter_state();
        log!("Foobar! Counter value: {}", counter_state.value);
    }

    pub fn only_for_owner(&mut self) -> Result<(), OwnableError> {
        self.only_owner()?;
        log!("Only for owner!");
        Ok(())
    }
}

#[casper]
impl HasTraits {
    pub fn multiple_impl_blocks_should_work() {
        log!("Multiple impl blocks work!");
    }
}

fn perform_test() {
    let contract_handle = ContractBuilder::<HasTraitsRef>::new()
        .default_create()
        .expect("should create contract");
    let trait1_handle =
        ContractHandle::<Trait1Ref>::from_address(contract_handle.contract_address());
    let counter_handle =
        ContractHandle::<CounterRef>::from_address(contract_handle.contract_address());
    {
        let greet_result: u64 = contract_handle
            .build_call()
            .call(|has_traits| has_traits.greet("World".into()))
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

#[casper(export)]
pub fn call() {
    log!("Hello");
    perform_test();
    log!("🎉 Success");
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{Counter, CounterExt, HasTraits, HasTraitsRef};

    use casper_sdk::{
        abi::{CasperABI, StructField},
        abi_generator,
        casper::native::{dispatch, dispatch_with, Environment},
        casper_executor_wasm_common::flags::EntryPointFlags,
        log,
        schema::{SchemaEntryPoint, SchemaType},
        ContractRef,
    };

    #[test]
    fn unit_test() {
        dispatch(|| {
            let mut has_traits = HasTraits::default();
            has_traits.increment();
        })
        .unwrap();
    }

    #[test]
    fn trait_has_schema() {
        // We can't attach methods to trait itself, but we can generate an "${TRAIT}Ext" struct and
        // attach extra information to it. let schema = Trait1::schema();
        let counter_schema = abi_generator::casper_collect_schema();

        assert_eq!(
            counter_schema.type_,
            SchemaType::Contract {
                state: "vm2_trait::CounterState".to_string(),
            }
        );

        // Order of entry point definitions is not guaranteed.
        assert_eq!(
            BTreeSet::from_iter(counter_schema.entry_points.clone()),
            BTreeSet::from_iter([
                SchemaEntryPoint {
                    name: "get_counter_value".to_string(),
                    arguments: vec![],
                    result: "U64".to_string(),
                    flags: EntryPointFlags::empty()
                },
                SchemaEntryPoint {
                    name: "get_counter_state".to_string(),
                    arguments: vec![],
                    result: "vm2_trait::CounterState".to_string(),
                    flags: EntryPointFlags::empty()
                },
                SchemaEntryPoint {
                    name: "decrement".to_string(),
                    arguments: vec![],
                    result: "()".to_string(),
                    flags: EntryPointFlags::empty()
                },
                SchemaEntryPoint {
                    name: "increment".to_string(),
                    arguments: vec![],
                    result: "()".to_string(),
                    flags: EntryPointFlags::empty()
                },
            ])
        );
    }

    #[test]
    fn schema_has_traits() {
        let schema = abi_generator::casper_collect_schema();

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
    /*#TODO fix native implementation
        #[test]
        fn foo() {
            let _ = dispatch_with(Environment::default(), || {
                super::perform_test();
            });

            log!("Success");
        }
    */
    #[test]
    fn bar() {
        let inst = <HasTraitsRef as ContractRef>::new();
        let _call_data = inst.get_counter_value();
    }
}
