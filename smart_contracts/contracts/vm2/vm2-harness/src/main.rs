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
    host, log, revert,
    types::{Address, CallError, ResultCode},
    Contract,
};

const INITIAL_GREETING: &str = "This is initial data set from a constructor";

#[derive(Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug)]
pub struct Harness {
    greeting: String,
    address_inside_constructor: Option<Address>,
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
            greeting: "Default value".to_string(),
            address_inside_constructor: None,
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
            greeting: format!("Hello, {who}!"),
            address_inside_constructor: Some(host::get_caller()),
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
            greeting: INITIAL_GREETING.to_string(),
            address_inside_constructor: Some(host::get_caller()),
        }
    }

    pub fn get_greeting(&self) -> &str {
        &self.greeting
    }

    pub fn set_greeting(&mut self, greeting: String) {
        log!("Saving greeting {}", greeting);
        self.greeting = greeting;
    }

    pub fn emit_unreachable_trap(&self) -> ! {
        panic!("unreachable");
    }

    pub fn emit_revert_with_data(&self) -> Result<(), CustomError> {
        revert!(Err(CustomError::Bar))
    }

    pub fn emit_revert_without_data(&self) -> ! {
        revert!()
    }

    pub fn get_address_inside_constructor(&self) -> Address {
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
}

#[casper(export)]
pub fn call() {
    log!("calling create");

    let session_caller = host::get_caller();
    assert_ne!(session_caller, [0; 32]);

    // Constructor without args

    {
        let contract_handle = Harness::create(HarnessRef::initialize()).expect("Should create");
        log!("success");
        log!("contract_address: {:?}", contract_handle.contract_address());

        // Verify that the address captured inside constructor is not the same as caller.
        // let get_address_inside_constructor = <HarnessRef as
        // ContractRef>::new().get_address_inside_constructor();
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

        let call_result = contract_handle
            .try_call(|harness| harness.emit_revert_with_data())
            .expect("Call succeed");
        assert_eq!(call_result.result, ResultCode::CalleeReverted);
        assert_eq!(call_result.into_return_value(), Err(CustomError::Bar),);

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
        let contract_handle = Harness::create(HarnessRef::constructor_with_args("World".into()))
            .expect("Should create");
        log!("success 2");
        log!("contract_address: {:?}", contract_handle.contract_address());

        let result = contract_handle
            .call(|harness| harness.get_greeting())
            .expect("Should call");
        assert_eq!(result, "Hello, World!".to_string(),);
    }

    {
        let error = Harness::create(HarnessRef::failing_constructor("World".to_string()))
            .expect_err(
                "
        Constructor that reverts should fail to create",
            );
        assert_eq!(error, CallError::CalleeReverted);

        let error = Harness::create(HarnessRef::trapping_constructor())
            .expect_err("Constructor that traps should fail to create");
        assert_eq!(error, CallError::CalleeTrapped);
    }
    log!("👋 Goodbye");
}

#[cfg(test)]
mod tests {

    use alloc::collections::{BTreeMap, BTreeSet};

    use casper_macros::selector;
    use casper_sdk::{
        host::native::dispatch,
        schema::{schema_helper, CasperSchema},
        Selector,
    };
    use vm_common::flags::EntryPointFlags;

    use super::*;

    #[test]
    fn test() {
        dispatch(|| {
            let args = ();
            schema_helper::dispatch("call", args);
        })
        .unwrap();
    }

    #[test]
    fn exports() {
        dbg!(schema_helper::list_exports());
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
