#![cfg_attr(target_arch = "wasm32", no_main)]

#[macro_use]
extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use borsh::{BorshDeserialize, BorshSerialize};
use casper_macros::{casper, CasperABI, CasperSchema, Contract};
use casper_sdk::{
    collections::Map,
    host::{self, Entity},
    log, revert,
    types::{Address, CallError, ResultCode},
    Contract,
};

const INITIAL_GREETING: &str = "This is initial data set from a constructor";
const BALANCES_PREFIX: &str = "b";

// #[casper(trait_definition)]
// pub trait TokenReceiver {
//     #[casper(selector = _)]
//     fn receive_tokens(&mut self, amount: u64);
// }

#[derive(Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug)]
pub struct Harness {
    counter: u64,
    greeting: String,
    address_inside_constructor: Option<Entity>,
    balances: Map<Entity, u64>,
}

#[repr(u32)]
#[derive(Debug, BorshSerialize, BorshDeserialize, PartialEq, CasperABI, Clone)]
#[borsh(use_discriminant = true)]
pub enum CustomError {
    Foo,
    Bar = 42,
    WithBody(String),
    Named { name: String, age: u64 },
}

impl Default for Harness {
    fn default() -> Self {
        Self {
            counter: 0,
            greeting: "Default value".to_string(),
            address_inside_constructor: None,
            balances: Map::new(BALANCES_PREFIX),
        }
    }
}
pub type Result2 = Result<(), CustomError>;

#[casper(contract)]
impl Harness {
    #[casper(constructor)]
    pub fn constructor_with_args(who: String) -> Self {
        log!("👋 Hello from constructor with args: {who}");
        Self {
            counter: 0,
            greeting: format!("Hello, {who}!"),
            address_inside_constructor: Some(host::get_caller()),
            balances: Map::new(BALANCES_PREFIX),
        }
    }

    #[casper(constructor)]
    pub fn failing_constructor(who: String) -> Self {
        log!("👋 Hello from failing constructor with args: {who}");
        revert!();
    }

    #[casper(constructor)]
    pub fn trapping_constructor() -> Self {
        log!("👋 Hello from trapping constructor");
        // TODO: Storage doesn't fork as of yet, need to integrate casper-storage crate and leverage
        // the tracking copy.
        panic!("This will revert the execution of this constructor and won't create a new package");
    }

    #[casper(constructor)]
    pub fn initialize() -> Self {
        log!("👋 Hello from constructor");
        Self {
            counter: 0,
            greeting: INITIAL_GREETING.to_string(),
            address_inside_constructor: Some(host::get_caller()),
            balances: Map::new(BALANCES_PREFIX),
        }
    }

    #[casper(constructor, payable)]
    pub fn payable_constructor() -> Self {
        log!(
            "👋 Hello from payable constructor value={}",
            host::get_value()
        );
        Self {
            counter: 0,
            greeting: INITIAL_GREETING.to_string(),
            address_inside_constructor: Some(host::get_caller()),
            balances: Map::new(BALANCES_PREFIX),
        }
    }

    #[casper(constructor, payable)]
    pub fn payable_failing_constructor() -> Self {
        log!(
            "👋 Hello from payable failign constructor value={}",
            host::get_value()
        );
        revert!();
    }

    #[casper(constructor, payable)]
    pub fn payable_trapping_constructor() -> Self {
        log!(
            "👋 Hello from payable trapping constructor value={}",
            host::get_value()
        );
        panic!("This will revert the execution of this constructor and won't create a new package")
    }

    pub fn get_greeting(&self) -> &str {
        &self.greeting
    }

    pub fn increment_counter(&mut self) {
        self.counter += 1;
    }

    pub fn counter(&self) -> u64 {
        self.counter
    }

    pub fn set_greeting(&mut self, greeting: String) {
        self.counter += 1;
        log!("Saving greeting {}", greeting);
        self.greeting = greeting;
    }

    pub fn emit_unreachable_trap(&mut self) -> ! {
        self.counter += 1;
        panic!("unreachable");
    }

    #[casper(revert_on_error)]
    pub fn emit_revert_with_data(&mut self) -> Result<(), CustomError> {
        log!("emit_revert_with_data state={:?}", self);
        log!(
            "Reverting with data before {counter}",
            counter = self.counter
        );
        self.counter += 1;
        log!(
            "Reverting with data after {counter}",
            counter = self.counter
        );
        // Here we can't use revert!() macro, as it explicitly calls `return` and does not involve writing the state again.
        Err(CustomError::Bar)
    }

    pub fn emit_revert_without_data(&mut self) -> ! {
        self.counter += 1;
        revert!()
    }

    pub fn get_address_inside_constructor(&self) -> Entity {
        self.address_inside_constructor
            .expect("Constructor was expected to be caller")
    }

    #[casper(revert_on_error)]
    pub fn should_revert_on_error(&self, flag: bool) -> Result2 {
        if flag {
            Err(CustomError::WithBody("Reverted".into()))
        } else {
            Ok(())
        }
    }

    #[allow(dead_code)]
    fn private_function_that_should_not_be_exported(&self) {
        log!("This function should not be callable from outside");
    }

    pub(crate) fn restricted_function_that_should_be_part_of_manifest(&self) {
        log!("This function should be callable from outside");
    }

    pub fn entry_point_without_state() {
        log!("This function does not require state");
    }

    pub fn entry_point_without_state_with_args_and_output(mut arg: String) -> String {
        log!("This function does not require state");
        arg.push_str("extra");
        arg
    }

    pub fn into_modified_greeting(mut self) -> String {
        self.greeting.push_str("!");
        self.greeting
    }

    pub fn into_greeting(self) -> String {
        self.greeting
    }

    #[casper(payable)]
    pub fn payable_entrypoint(&mut self) -> Result<(), CustomError> {
        log!("This is a payable entrypoint value={}", host::get_value());
        Ok(())
    }

    #[casper(payable, revert_on_error)]
    pub fn payable_failing_entrypoint(&self) -> Result<(), CustomError> {
        log!(
            "This is a payable entrypoint with value={}",
            host::get_value()
        );
        if host::get_value() == 123 {
            Err(CustomError::Foo)
        } else {
            Ok(())
        }
    }

    #[casper(payable, revert_on_error)]
    pub fn deposit(&mut self, balance_before: u64) -> Result<(), CustomError> {
        let caller = host::get_caller();
        let value = host::get_value();

        if value == 0 {
            return Err(CustomError::WithBody(
                "Value should be greater than 0".into(),
            ));
        }

        assert_eq!(
            balance_before
                .checked_sub(value)
                .expect("Balance before should be larger or equal to the value"),
            host::get_balance_of(&caller),
            "Balance mismatch; token transfer should happen before a contract call"
        );

        log!("Depositing {value} from {caller:?}");
        let current_balance = self.balances.get(&caller).unwrap_or(0);
        self.balances.insert(&caller, &(current_balance + value));
        Ok(())
    }

    #[casper(payable, revert_on_error)]
    pub fn withdraw(&mut self, amount: u64) -> Result<(), CustomError> {
        let caller = host::get_caller();
        log!("Withdrawing {amount} from {caller:?}");
        let current_balance = self.balances.get(&caller).unwrap_or(0);
        if current_balance < amount {
            return Err(CustomError::WithBody("Insufficient balance".into()));
        }
        if !host::casper_transfer(&caller, amount) {
            return Err(CustomError::WithBody("Transfer failed".into()));
        }
        self.balances.insert(&caller, &(current_balance - amount));
        Ok(())
    }

    pub fn balance(&self) -> u64 {
        if host::get_value() != 0 {
            panic!("This function is not payable");
        }
        let caller = host::get_caller();
        self.balances.get(&caller).unwrap_or(0)
    }
}

#[casper(export)]
pub fn call() {
    use casper_sdk::ContractBuilder;

    log!("calling create");

    let session_caller = host::get_caller();
    assert_ne!(session_caller, Entity::Account([0; 32]));

    // Constructor without args

    {
        let contract_handle = Harness::create(0, HarnessRef::initialize()).expect("Should create");
        log!("success");
        log!("contract_address: {:?}", contract_handle.contract_address());

        // Verify that the address captured inside constructor is not the same as caller.
        let greeting_result = contract_handle
            .call(|harness| harness.get_greeting())
            .expect("Should call");
        log!("Getting greeting: {greeting_result}");
        assert_eq!(greeting_result, INITIAL_GREETING);

        let () = contract_handle
            .call(|harness| harness.set_greeting("Foo".into()))
            .expect("Should call");

        log!("New greeting saved");
        let greeting_result = contract_handle
            .call(|harness| harness.get_greeting())
            .expect("Should call");
        assert_eq!(greeting_result, "Foo");

        log!("Emitting unreachable trap");

        let call_result = contract_handle.call(|harness| harness.emit_unreachable_trap());
        assert_eq!(call_result, Err(CallError::CalleeTrapped));

        log!("Trap recovered");

        {
            let counter_value_before = contract_handle
                .call(|harness| harness.counter())
                .expect("Should call");

            // increase counter
            let () = contract_handle
                .call(|harness| harness.increment_counter())
                .expect("Should call");

            let counter_value_after = contract_handle
                .call(|harness| harness.counter())
                .expect("Should call");

            assert_eq!(counter_value_before + 1, counter_value_after);
        }

        {
            let counter_value_before = contract_handle
                .call(|harness| harness.counter())
                .expect("Should call");

            let call_result = contract_handle
                .try_call(|harness| harness.emit_revert_with_data())
                .expect("Call succeed");

            assert_eq!(call_result.result, ResultCode::CalleeReverted);
            assert_eq!(call_result.into_return_value(), Err(CustomError::Bar),);

            let counter_value_after = contract_handle
                .call(|harness| harness.counter())
                .expect("Should call");

            assert_eq!(counter_value_before, counter_value_after);
        }

        log!("Revert with data success");

        let call_result = contract_handle
            .try_call(|harness| harness.emit_revert_without_data())
            .expect("Call succeed");
        assert_eq!(call_result.result, ResultCode::CalleeReverted);
        assert_eq!(call_result.data, None);

        log!("Revert without data success");

        let call_result = contract_handle
            .try_call(|harness| harness.should_revert_on_error(false))
            .expect("Call succeed");
        assert!(!call_result.did_revert());
        assert_eq!(call_result.into_return_value(), Ok(()));

        log!("Revert on error success (ok case)");

        let call_result = contract_handle
            .try_call(|harness| harness.should_revert_on_error(true))
            .expect("Call succeed");
        assert!(call_result.did_revert());
        assert_eq!(
            call_result.into_return_value(),
            Err(CustomError::WithBody("Reverted".to_string()))
        );

        log!("Revert on error success (err case)");
        // let should_revert_on_error: TypedCall<(bool,), Result<(), CustomError>> =
        //     TypedCall::new(contract_address, selector!("should_revert_on_error"));
        // let result = should_revert_on_error.call((false,));
        // assert!(!result.did_revert());

        // let result = should_revert_on_error.call((true,));
        // assert!(result.did_revert());
        // assert_eq!(
        //     result.into_return_value(),
        //     Err(CustomError::WithBody("Reverted".to_string()))
        // );
    }

    // Constructor with args

    {
        let contract_handle = Harness::create(0, HarnessRef::constructor_with_args("World".into()))
            .expect("Should create");
        log!("success 2");
        log!("contract_address: {:?}", contract_handle.contract_address());

        let result = contract_handle
            .call(|harness| harness.get_greeting())
            .expect("Should call");
        assert_eq!(result, "Hello, World!".to_string(),);
    }

    {
        let error = Harness::create(0, HarnessRef::failing_constructor("World".to_string()))
            .expect_err(
                "
        Constructor that reverts should fail to create",
            );
        assert_eq!(error, CallError::CalleeReverted);

        let error = Harness::create(0, HarnessRef::trapping_constructor())
            .expect_err("Constructor that traps should fail to create");
        assert_eq!(error, CallError::CalleeTrapped);
    }

    //
    // Check payable entrypoints
    //

    {
        let contract_handle = ContractBuilder::<Harness>::new()
            .with_value(1)
            .create(|| HarnessRef::payable_constructor())
            .expect("Should create");
        Harness::create(0, HarnessRef::constructor_with_args("Payable".to_string()))
            .expect("Should create");
        assert_eq!(contract_handle.balance(), 1);

        log!("success 2");
        log!("contract_address: {:?}", contract_handle.contract_address());

        // Transferring 500 motes before payable entrypoint is executed

        let result_1 = contract_handle
            .build_call()
            .with_value(500)
            .call(|harness| harness.payable_entrypoint())
            .expect("Should call");
        assert_eq!(result_1, Ok(()));

        // Transferring 499 motes before payable entrypoint is executed

        let result_2 = contract_handle
            .build_call()
            .with_value(499)
            .call(|harness| harness.payable_entrypoint())
            .expect("Should call");
        assert_eq!(result_2, Ok(()));

        // Check balance after payable constructor and two successful calls
        assert_eq!(contract_handle.balance(), 1 + 500 + 499);

        let result_3 = contract_handle
            .build_call()
            .with_value(123)
            .call(|harness| harness.payable_failing_entrypoint())
            .expect("Should call");
        assert_eq!(result_3, Err(CustomError::Foo));
        // Check balance after failed call, should be the same as before
        assert_eq!(contract_handle.balance(), 1 + 500 + 499);
    }

    // Deposit and withdraw
    // 1. wasm (caller = A, callee = B)
    //   2. create (caller = B, callee = C)
    //   3. call (caller = B, callee = C)
    //     4. create (caller = C, callee = D)
    //     5. call (caller = C, callee = D)

    {
        log!("Current caller {:?}", host::get_caller());

        let contract_handle = ContractBuilder::<Harness>::new()
            .with_value(0)
            .create(|| HarnessRef::payable_constructor())
            .expect("Should create");

        let caller = host::get_caller();

        {
            let account_balance_1 = host::get_balance_of(&caller);
            contract_handle
                .build_call()
                .with_value(100)
                .call(|harness| harness.deposit(account_balance_1))
                .expect("Should call")
                .expect("Should succeed");
            let account_balance_2 = host::get_balance_of(&caller);
            assert_eq!(account_balance_2, account_balance_1 - 100);

            contract_handle
                .build_call()
                .with_value(25)
                .call(|harness| harness.deposit(account_balance_2))
                .expect("Should call")
                .expect("Should succeed");

            let account_balance_after = host::get_balance_of(&caller);
            assert_eq!(account_balance_after, account_balance_1 - 125);
        }

        let current_contract_balance = contract_handle
            .build_call()
            .call(|harness| harness.balance())
            .expect("Should call");
        assert_eq!(current_contract_balance, 100 + 25);

        {
            let account_balance_before = host::get_balance_of(&caller);
            contract_handle
                .build_call()
                .call(|harness| harness.withdraw(50))
                .expect("Should call")
                .expect("Should succeed");
            let account_balance_after = host::get_balance_of(&caller);
            assert_ne!(account_balance_after, account_balance_before);
            assert_eq!(account_balance_after, account_balance_before + 50);

            let current_deposit_balance = contract_handle
                .build_call()
                .call(|harness| harness.balance())
                .expect("Should call");
            assert_eq!(current_deposit_balance, 100 + 25 - 50);

            assert_eq!(contract_handle.balance(), 100 + 25 - 50);
        }
    }

    log!("👋 Goodbye");
}

#[cfg(test)]
mod tests {

    use alloc::collections::{BTreeMap, BTreeSet};

    use casper_macros::selector;
    use casper_sdk::{
        host::native::{self, dispatch},
        schema::CasperSchema,
        Selector,
    };
    use vm_common::flags::EntryPointFlags;

    use super::*;

    #[test]
    fn test() {
        dispatch(|| {
            native::call_export("call");
        })
        .unwrap();
    }

    #[test]
    fn exports() {
        let exports = native::list_exports()
            .into_iter()
            .map(|e| e.name)
            .collect::<Vec<_>>();
        assert_eq!(exports, vec!["call"]);
    }

    #[test]
    fn compile_time_schema() {
        let schema = Harness::schema();
        dbg!(&schema);
        // println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    }

    #[test]
    fn should_greet() {
        assert_eq!(Harness::name(), "Harness");
        let mut flipper = Harness::constructor_with_args("Hello".into());
        assert_eq!(flipper.get_greeting(), "Hello"); // TODO: Initializer
        flipper.set_greeting("Hi".into());
        assert_eq!(flipper.get_greeting(), "Hi");
    }

    #[test]
    fn unittest() {
        dispatch(|| {
            let mut foo = Harness::initialize();
            assert_eq!(foo.get_greeting(), INITIAL_GREETING);
            foo.set_greeting("New greeting".to_string());
            assert_eq!(foo.get_greeting(), "New greeting");
        })
        .unwrap();
    }

    #[test]
    fn list_of_constructors() {
        let schema = Harness::schema();
        let constructors: BTreeSet<_> = schema
            .entry_points
            .iter()
            .filter_map(|e| {
                if e.flags.contains(EntryPointFlags::CONSTRUCTOR) {
                    Some(e.name.as_str())
                } else {
                    None
                }
            })
            .collect();
        let expected = BTreeSet::from_iter([
            "constructor_with_args",
            "failing_constructor",
            "trapping_constructor",
            "initialize",
        ]);

        assert_eq!(constructors, expected);
    }

    #[test]
    fn check_schema_selectors() {
        let schema = Harness::schema();

        let schema_mapping: BTreeMap<_, _> = schema
            .entry_points
            .iter()
            .map(|e| (e.name.as_str(), e.selector))
            .collect();

        let manifest = &Harness::MANIFEST;

        let manifest_selectors: BTreeSet<u32> = manifest.iter().map(|e| e.selector).collect();

        assert_eq!(schema_mapping["constructor_with_args"], 4116419170,);
        assert!(manifest_selectors.contains(&4116419170));
    }

    #[test]
    fn verify_check_private_and_public_methods() {
        dispatch(|| {
            Harness::default().private_function_that_should_not_be_exported();
            Harness::default().restricted_function_that_should_be_part_of_manifest();
        })
        .expect("No trap");

        let manifest = &Harness::MANIFEST;
        const PRIVATE_SELECTOR: Selector =
            selector!("private_function_that_should_not_be_exported");
        const PUB_CRATE_SELECTOR: Selector =
            selector!("restricted_function_that_should_be_part_of_manifest");
        assert!(
            manifest
                .iter()
                .find(|e| e.selector == PRIVATE_SELECTOR.get())
                .is_none(),
            "This entry point should not be part of manifest"
        );
        assert!(
            manifest
                .iter()
                .find(|e| e.selector == PUB_CRATE_SELECTOR.get())
                .is_some(),
            "This entry point should be part of manifest"
        );

        let schema = Harness::schema();
        assert!(
            schema
                .entry_points
                .iter()
                .find(|e| e.selector == PRIVATE_SELECTOR.get())
                .is_none(),
            "This entry point should not be part of schema"
        );
        assert!(
            schema
                .entry_points
                .iter()
                .find(|e| e.selector == PUB_CRATE_SELECTOR.get())
                .is_some(),
            "This entry point should be part ozf schema"
        );
    }

    #[test]
    fn foo() {
        assert_eq!(Harness::default().into_greeting(), "Default value");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    panic!("Execute \"cargo test\" to test the contract, \"cargo build\" to build it");
}
