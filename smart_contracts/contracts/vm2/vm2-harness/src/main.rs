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
use casper_macros::{casper, CasperABI, CasperSchema, Contract};
use casper_sdk::{
    host::{self, Alloc},
    log, revert,
    sys::CreateResult,
    types::{Address, CallError, ResultCode},
    Contract,
};

const INITIAL_GREETING: &str = "This is initial data set from a constructor";

#[derive(Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug)]
struct Greeter {
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
}

impl Default for Greeter {
    fn default() -> Self {
        Self {
            greeting: "Default value".to_string(),
            address_inside_constructor: None,
        }
    }
}
pub type Result2 = Result<(), CustomError>;

#[casper(entry_points)]
impl Greeter {
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
    pub fn should_revert_on_error(&self) -> Result2 {
        Err(CustomError::WithBody("Reverted".into()))
    }
}

struct TypedCall<Args: BorshSerialize, Ret: BorshDeserialize> {
    address: Address,
    name: &'static str,
    _marker: PhantomData<fn(Args) -> Ret>,
}

struct CallResult<Ret: BorshDeserialize> {
    data: Option<Vec<u8>>,
    result_code: ResultCode,
    _marker: PhantomData<Ret>,
}

impl<Ret: BorshDeserialize> CallResult<Ret> {
    fn into_result(self) -> Ret {
        match self.result_code {
            ResultCode::Success | ResultCode::CalleeReverted => {
                borsh::from_slice::<Ret>(&self.data.unwrap()).unwrap()
            }
            ResultCode::CalleeTrapped => panic!("CalleeTrapped"),
            ResultCode::CalleeGasDepleted => panic!("CalleeGasDepleted"),
            ResultCode::Unknown => panic!("Unknown"),
        }
    }
    fn did_revert(&self) -> bool {
        self.result_code == ResultCode::CalleeReverted
    }
}
impl<Args: BorshSerialize, Ret: BorshDeserialize> TypedCall<Args, Ret> {
    fn new(address: Address, name: &'static str) -> Self {
        Self {
            address,
            name,
            _marker: PhantomData,
        }
    }
    fn call(&self, args: Args) -> CallResult<Ret> {
        let args_data = borsh::to_vec(&args).unwrap();
        let (maybe_output_data, result_code) =
            host::casper_call(&self.address, 0, self.name, &args_data);
        CallResult::<Ret> {
            data: maybe_output_data,
            result_code,
            _marker: PhantomData,
        }
    }
}

#[casper(export)]
pub fn call() {
    log!("calling create");

    let session_caller = host::get_caller();
    assert_ne!(session_caller, [0; 32]);

    // Constructor without args

    match Greeter::create(Some("initialize"), None) {
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
            let get_address_inside_constructor: TypedCall<(), Address> =
                TypedCall::new(contract_address, "get_address_inside_constructor");
            let result = get_address_inside_constructor.call(()).into_result();
            assert_ne!(result, session_caller);

            let get_greeting: TypedCall<(), String> =
                TypedCall::new(contract_address, "get_greeting");
            let result = get_greeting.call(()).into_result();
            assert_eq!(result, INITIAL_GREETING);

            let call_1 = "set_greeting";
            let input_data_1: (String,) = ("Foo".into(),);
            let (maybe_data_1, result_code_1) = host::casper_call(
                &contract_address,
                0,
                call_1,
                &borsh::to_vec(&input_data_1).unwrap(),
            );
            log!("{call_1:?} result={result_code_1:?}");

            let call_2 = "get_greeting";
            let (maybe_data_2, result_code_2) =
                host::casper_call(&contract_address, 0, call_2, &[]);
            log!("{call_2:?} result={result_code_2:?}");
            assert_eq!(
                borsh::from_slice::<String>(&maybe_data_2.as_ref().expect("return value")).unwrap(),
                "Foo".to_string()
            );

            let call_3 = "emit_unreachable_trap";
            let (maybe_data_3, result_code_3) =
                host::casper_call(&contract_address, 0, call_3, &[]);
            assert_eq!(maybe_data_3, None);
            assert_eq!(result_code_3, ResultCode::CalleeTrapped);

            let call_4: &str = "emit_revert_with_data";
            let (maybe_data_4, maybe_result_4) =
                host::casper_call(&contract_address, 0, call_4, &[]);
            log!("{call_4:?} result={maybe_data_4:?}");
            assert_eq!(maybe_result_4, ResultCode::CalleeReverted);
            assert_eq!(
                borsh::from_slice::<Result<(), CustomError>>(
                    &maybe_data_4.as_ref().expect("return value")
                )
                .unwrap(),
                Err(CustomError::Bar),
            );

            let call_5: &str = "emit_revert_without_data";
            let (maybe_data_5, maybe_result_5) =
                host::casper_call(&contract_address, 0, call_5, &[]);
            log!("{call_5:?} result={maybe_data_5:?}");
            assert_eq!(maybe_result_5, ResultCode::CalleeReverted);
            assert_eq!(maybe_data_5, None);

            let get_greeting: TypedCall<(), Result<(), CustomError>> =
                TypedCall::new(contract_address, "should_revert_on_error");
            let result = get_greeting.call(());
            assert!(result.did_revert());
            assert_eq!(
                result.into_result(),
                Err(CustomError::WithBody("Reverted".to_string()))
            );
        }
        Err(error) => {
            log!("error {:?}", error);
        }
    }

    // Constructor with args

    let args = ("World".to_string(),);
    match Greeter::create(
        Some("constructor_with_args"),
        Some(&borsh::to_vec(&args).unwrap()),
    ) {
        Ok(CreateResult {
            package_address,
            contract_address,
            version,
        }) => {
            log!("success 2");
            log!("package_address: {:?}", package_address);
            log!("contract_address: {:?}", contract_address);
            log!("version: {:?}", version);

            let call_0 = "get_greeting";
            let (maybe_data_0, result_code_0) =
                host::casper_call(&contract_address, 0, call_0, &[]);
            log!("{call_0:?} result={result_code_0:?}");
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

    let error = Greeter::create(
        Some("failing_constructor"),
        Some(&borsh::to_vec(&args).unwrap()),
    )
    .expect_err("Constructor that reverts should fail to create");
    assert_eq!(error, CallError::CalleeReverted);

    let error = Greeter::create(Some("trapping_constructor"), None)
        .expect_err("Constructor that traps should fail to create");
    assert_eq!(error, CallError::CalleeTrapped);

    log!("👋 Goodbye");
}

#[cfg(test)]
mod tests {

    use alloc::collections::BTreeSet;
    use borsh::{schema::BorshSchemaContainer, BorshSchema};
    use casper_sdk::{
        schema::{schema_helper, CasperSchema},
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
        let schema = Greeter::schema();
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
        let mut foo = Greeter::initialize();
        assert_eq!(foo.get_greeting(), INITIAL_GREETING);
        foo.set_greeting("New greeting".to_string());
        assert_eq!(foo.get_greeting(), "New greeting");
    }

    #[test]
    fn list_of_constructors() {
        let constructors: BTreeSet<_> = Greeter::schema()
            .entry_points
            .into_iter()
            .filter_map(|e| {
                if e.flags.contains(EntryPointFlags::CONSTRUCTOR) {
                    Some(e.name)
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
}
