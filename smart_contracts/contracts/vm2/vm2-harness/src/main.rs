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

const INITIAL_GREETING: &str = "This is initial data set from a constructor";

#[derive(Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug)]
struct Harness {
    greeting: String,
    address_inside_constructor: Option<Address>,
}

#[repr(u32)]
#[derive(Debug, BorshSerialize, BorshDeserialize, PartialEq, CasperABI)]
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

#[casper(entry_points)]
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

struct TypedCall<Args: BorshSerialize, Ret: BorshDeserialize> {
    address: Address,
    selector: Selector,
    _marker: PhantomData<fn(Args) -> Ret>,
}

impl<Args: BorshSerialize, Ret: BorshDeserialize> TypedCall<Args, Ret> {
    const fn new(address: Address, selector: Selector) -> Self {
        Self {
            address,
            selector,
            _marker: PhantomData,
        }
    }
    fn call(&self, args: Args) -> CallResult<Ret> {
        let args_data = borsh::to_vec(&args).unwrap();
        let (maybe_output_data, result_code) =
            host::casper_call(&self.address, 0, self.selector, &args_data);
        CallResult::<Ret> {
            data: maybe_output_data,
            result: result_code,
            marker: PhantomData,
        }
    }
}

#[casper(export)]
pub fn call() {
    log!("calling create");

    let session_caller = host::get_caller();
    assert_ne!(session_caller, [0; 32]);

    // Constructor without args

    match Harness::create(Harness_initialize {}) {
        Ok(CreateResult {
            package_address,
            contract_address,
            version,
        }) => {
            log!("success");
            log!("package_address: {:?}", package_address);
            log!("contract_address: {:?}", contract_address);
            log!("version: {:?}", version);

            // Verify that the address captured inside constructor is not the same as caller.
            const SELECTOR: Selector = selector!("get_address_inside_constructor");
            let get_address_inside_constructor: TypedCall<(), Address> =
                TypedCall::new(contract_address, SELECTOR);
            let result = get_address_inside_constructor.call(()).into_return_value();
            assert_ne!(result, session_caller);

            const GET_GREETING: Selector = selector!("get_greeting");
            const SET_GREETING: Selector = selector!("set_greeting");

            let get_greeting: TypedCall<(), String> =
                TypedCall::new(contract_address, GET_GREETING);
            let result = get_greeting.call(()).into_return_value();
            assert_eq!(result, INITIAL_GREETING);

            let call_1: Selector = selector!("set_greeting");
            let input_data_1: (String,) = ("Foo".into(),);
            let (maybe_data_1, result_code_1) = host::casper_call(
                &contract_address,
                0,
                call_1,
                &borsh::to_vec(&input_data_1).unwrap(),
            );
            log!("{call_1:?} result={result_code_1:?}");

            let (maybe_data_2, result_code_2) =
                host::casper_call(&contract_address, 0, GET_GREETING, &[]);
            log!("get_greeting (selector:{GET_GREETING:?}) result={result_code_2:?}");
            assert_eq!(
                borsh::from_slice::<String>(&maybe_data_2.as_ref().expect("return value")).unwrap(),
                "Foo".to_string()
            );

            const CALL_3: Selector = selector!("emit_unreachable_trap");
            let (maybe_data_3, result_code_3) =
                host::casper_call(&contract_address, 0, CALL_3, &[]);
            assert_eq!(maybe_data_3, None);
            assert_eq!(result_code_3, ResultCode::CalleeTrapped);

            const CALL_4: Selector = selector!("emit_revert_with_data");
            let (maybe_data_4, maybe_result_4) =
                host::casper_call(&contract_address, 0, CALL_4, &[]);

            log!("{CALL_4:?} result={maybe_data_4:?}");
            assert_eq!(maybe_result_4, ResultCode::CalleeReverted);
            assert_eq!(
                borsh::from_slice::<Result<(), CustomError>>(
                    &maybe_data_4.as_ref().expect("return value")
                )
                .unwrap(),
                Err(CustomError::Bar),
            );

            const CALL_5: Selector = selector!("emit_revert_without_data");
            let (maybe_data_5, maybe_result_5) =
                host::casper_call(&contract_address, 0, CALL_5, &[]);
            log!("{CALL_5:?} result={maybe_data_5:?}");
            assert_eq!(maybe_result_5, ResultCode::CalleeReverted);
            assert_eq!(maybe_data_5, None);

            let should_revert_on_error: TypedCall<(bool,), Result<(), CustomError>> =
                TypedCall::new(contract_address, selector!("should_revert_on_error"));
            let result = should_revert_on_error.call((false,));
            assert!(!result.did_revert());
            assert_eq!(result.into_return_value(), Ok(()));

            let result = should_revert_on_error.call((true,));
            assert!(result.did_revert());
            assert_eq!(
                result.into_return_value(),
                Err(CustomError::WithBody("Reverted".to_string()))
            );
        }
        Err(error) => {
            log!("error {:?}", error);
        }
    }

    // Constructor with args

    let args = ("World".to_string(),);
    match Harness::create(Harness_constructor_with_args {
        who: "World".to_string(),
    }) {
        Ok(CreateResult {
            package_address,
            contract_address,
            version,
        }) => {
            log!("success 2");
            log!("package_address: {:?}", package_address);
            log!("contract_address: {:?}", contract_address);
            log!("version: {:?}", version);

            let (maybe_data_0, result_code_0) =
                host::casper_call(&contract_address, 0, Harness_get_greeting::SELECTOR, &[]);
            log!("get_greeting result={result_code_0:?}");
            assert_eq!(
                borsh::from_slice::<String>(&maybe_data_0.as_ref().expect(
                    "return
    value"
                ))
                .unwrap(),
                "Hello, World!".to_string(),
            );
        }
        Err(error) => {
            log!("error {:?}", error);
        }
    }

    let args = ("World".to_string(),);

    let error = Harness::create(Harness_failing_constructor {
        who: "World".to_string(),
    })
    .expect_err("Constructor that reverts should fail to create");
    assert_eq!(error, CallError::CalleeReverted);

    let error = Harness::create(Harness_trapping_constructor {})
        .expect_err("Constructor that traps should fail to create");
    assert_eq!(error, CallError::CalleeTrapped);

    log!("👋 Goodbye");
}

#[cfg(test)]
mod tests {

    use core::slice;

    use alloc::collections::{BTreeMap, BTreeSet};
    use borsh::{schema::BorshSchemaContainer, BorshSchema};
    use casper_sdk::{
        host::native::{dispatch_with, Stub},
        schema::{schema_helper, CasperSchema},
        sys::Manifest,
        Contract,
    };
    use vm_common::flags::EntryPointFlags;

    use super::*;

    #[test]
    fn test() {
        let args = ();
        schema_helper::dispatch("call", args);
    }

    #[test]
    fn exports() {
        dbg!(schema_helper::list_exports());
    }

    #[test]
    fn compile_time_schema() {
        let schema = Harness::schema();
        dbg!(&schema);
        println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    }

    #[test]
    fn should_greet() {
        // assert_eq!(Greeter::name(), "Greeter");
        // let mut flipper = Greeter::new();
        // assert_eq!(flipper.get_greeting(), ""); // TODO: Initializer
        // flipper.set_greeting("Hi".into());
        // assert_eq!(flipper.get_greeting(), "Hi");
    }

    #[test]
    fn unittest() {
        let mut foo = Harness::initialize();
        assert_eq!(foo.get_greeting(), INITIAL_GREETING);
        foo.set_greeting("New greeting".to_string());
        assert_eq!(foo.get_greeting(), "New greeting");
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

        let manifest = Harness::__casper_manifest();
        let manifest_entrypoints =
            unsafe { slice::from_raw_parts(manifest.entry_points, manifest.entry_points_size) };

        let manifest_selectors: BTreeSet<u32> =
            manifest_entrypoints.iter().map(|e| e.selector).collect();

        assert_eq!(schema_mapping["constructor_with_args"], 4116419170,);
        assert!(manifest_selectors.contains(&4116419170));
    }

    #[test]
    fn verify_check_private_and_public_methods() {
        dispatch_with(Stub::default(), || {
            Harness::default().private_function_that_should_not_be_exported();
            Harness::default().restricted_function_that_should_be_part_of_manifest();
        })
        .expect("No trap");

        const MANIFEST: Manifest = Harness::__casper_manifest();
        const PRIVATE_SELECTOR: Selector =
            selector!("private_function_that_should_not_be_exported");
        const PUB_CRATE_SELECTOR: Selector =
            selector!("restricted_function_that_should_be_part_of_manifest");
        let entry_points =
            unsafe { slice::from_raw_parts(MANIFEST.entry_points, MANIFEST.entry_points_size) };
        assert!(
            entry_points
                .iter()
                .find(|e| e.selector == PRIVATE_SELECTOR.get())
                .is_none(),
            "This entry point should not be part of manifest"
        );
        assert!(
            entry_points
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
