#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_macros::casper;
use casper_sdk::{
    collections::{sorted_vector::SortedVector, Map},
    host::{self, Entity},
    log,
    prelude::*,
    types::Address,
    ContractBuilder, ContractHandle,
};

pub const GREET_RETURN_VALUE: u64 = 123456789;

#[casper]
pub trait HasFallback {
    #[casper(fallback)]
    fn this_is_fallback_method(&self) {
        log!("Fallback called with value={}", host::get_value());
    }
}

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

    fn get_counter_state(&self) -> CounterState {
        self.counter_state().clone()
    }

    #[casper(private)]
    fn counter_state(&self) -> &CounterState;

    #[casper(private)]
    fn counter_state_mut(&mut self) -> &mut CounterState;
}

#[casper]
pub struct OwnableState {
    owner: Option<Entity>,
}

impl Default for OwnableState {
    fn default() -> Self {
        Self {
            owner: Some(casper_sdk::host::get_caller()),
        }
    }
}

#[casper]
pub enum OwnableError {
    /// The caller is not authorized to perform the action.
    NotAuthorized,
}

#[casper]
pub trait Ownable {
    #[casper(private)]
    fn state(&self) -> &OwnableState;
    #[casper(private)]
    fn state_mut(&mut self) -> &mut OwnableState;

    fn only_owner(&self) -> Result<(), OwnableError> {
        let caller = casper_sdk::host::get_caller();
        match self.state().owner {
            Some(owner) if caller != owner => {
                return Err(OwnableError::NotAuthorized);
            }
            None => {
                return Err(OwnableError::NotAuthorized);
            }
            Some(_owner) => {}
        }
        Ok(())
    }

    fn transfer_ownership(&mut self, new_owner: Entity) -> Result<(), OwnableError> {
        self.only_owner()?;
        self.state_mut().owner = Some(new_owner);
        Ok(())
    }

    fn owner(&self) -> Option<Entity> {
        self.state().owner
    }

    fn renounce_ownership(&mut self) -> Result<(), OwnableError> {
        self.only_owner()?;
        self.state_mut().owner = None;
        Ok(())
    }
}

#[casper]
pub struct AccessControlState {
    roles: Map<Address, SortedVector<[u8; 32]>>,
}

impl Default for AccessControlState {
    fn default() -> Self {
        Self {
            roles: Map::new("roles"),
        }
    }
}

#[casper]
pub trait AccessControl {
    #[casper(private)]
    fn state(&self) -> &AccessControlState;
    #[casper(private)]
    fn state_mut(&mut self) -> &mut AccessControlState;

    fn has_role(&self, account: Address, role: [u8; 32]) -> bool {
        match self.state().roles.get(&account) {
            Some(roles) => roles.contains(&role),
            None => false,
        }
    }

    fn grant_role(&mut self, account: Address, role: [u8; 32]) {
        // let roles = self.state_mut().roles.entry(account).or_insert_with(Vec::new);
        match self.state_mut().roles.get(&account) {
            Some(mut roles) => {
                if roles.contains(&role) {
                    return;
                }
                roles.push(role);
            }
            None => {
                let mut roles =
                    SortedVector::new(format!("roles-{}", base16::encode_lower(&account)));
                roles.push(role);
                self.state_mut().roles.insert(&account, &roles);
            }
        }
    }

    fn revoke_role(&mut self, account: Address, role: [u8; 32]) {
        if let Some(mut roles) = self.state_mut().roles.get(&account) {
            roles.retain(|r| r != &role);
        }
    }
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

#[casper]
impl HasFallback for HasTraits {}

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

#[casper]
impl Ownable for HasTraits {
    fn state(&self) -> &OwnableState {
        &self.ownable_state
    }
    fn state_mut(&mut self) -> &mut OwnableState {
        &mut self.ownable_state
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
    use crate::{CounterExt, HasTraits, HasTraitsRef, Trait1};

    use alloc::collections::BTreeSet;
    use casper_macros::selector;
    use casper_sdk::{
        abi::StructField,
        host::{
            self,
            native::{dispatch_with, Environment},
        },
        log,
        schema::{CasperSchema, SchemaEntryPoint, SchemaType},
        Contract, ContractRef,
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
    // use casper_sdk::abi::CasperABI;
    #[test]
    fn unit_test() {
        let mut has_traits = HasTraits::default();
        has_traits.increment();
    }

    #[test]
    fn entrypoints() {
        let entrypoints = HasTraitsRef::ENTRY_POINTS;

        let mut i = 0;
        for entrypoint in entrypoints.to_vec().into_iter().flatten() {
            if entrypoint.selector == 0 && entrypoint.flags == EntryPointFlags::FALLBACK.bits() {
                i += 1;
            }
        }
        assert_eq!(i, 1, "Exactly one fallback method");
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
                    selector: Some(selector!("get_counter_value").get()),
                    arguments: vec![],
                    result: "U64".to_string(),
                    flags: EntryPointFlags::empty()
                },
                SchemaEntryPoint {
                    name: "get_counter_state".to_string(),
                    selector: Some(selector!("get_counter_state").get()),
                    arguments: vec![],
                    result: "vm2_trait::CounterState".to_string(),
                    flags: EntryPointFlags::empty()
                },
                SchemaEntryPoint {
                    name: "decrement".to_string(),
                    selector: Some(selector!("decrement").get()),
                    arguments: vec![],
                    result: "()".to_string(),
                    flags: EntryPointFlags::empty()
                },
                SchemaEntryPoint {
                    name: "increment".to_string(),
                    selector: Some(selector!("increment").get()),
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

        let fallback = schema
            .entry_points
            .iter()
            .filter_map(|e| if e.name == "fallback" { Some(e) } else { None })
            .next()
            .expect("Fallback method present in schema");

        assert_eq!(fallback.selector, None);
        assert_eq!(fallback.flags, EntryPointFlags::FALLBACK);
    }

    #[test]
    fn foo_with_custom_constructor() {
        let _ret = dispatch_with(Environment::default(), || {
            let constructor = super::HasTraitsRef::new(5);

            let has_traits_handle = HasTraits::create(0, constructor).expect("Constructor works");

            let value = host::call(
                &has_traits_handle.contract_address(),
                0,
                super::CounterRef::new().get_counter_value(),
            )
            .expect("Call");

            assert_eq!(value.into_result(), Ok(5));
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

    #[test]
    fn bar() {
        let inst = <HasTraitsRef as ContractRef>::new();
        let _call_data = inst.get_counter_value();
    }
}
