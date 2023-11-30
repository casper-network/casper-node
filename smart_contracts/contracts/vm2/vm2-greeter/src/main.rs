#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

#[macro_use]
extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use borsh::{BorshDeserialize, BorshSerialize};
// use borsh_derive::BorshSerialize;
use casper_macros::{casper, Contract};
use casper_sdk::{
    host::{self, CreateResult, ResultCode},
    log, revert, Value,
};

#[derive(Contract)]
struct Greeter {
    greeting: Value<String>,
}

#[derive(Debug, BorshSerialize, BorshDeserialize, PartialEq)]
pub enum CustomError {
    Foo,
    Bar,
}

#[casper(entry_points)]
impl Greeter {
    pub fn get_greeting(&self) -> String {
        self.greeting
            .read()
            .expect("should read value")
            .unwrap_or_default()
    }
    pub fn set_greeting(&mut self, greeting: String) {
        log!("Saving greeting {}", greeting);
        self.greeting.write(greeting).unwrap();
    }

    pub fn emit_unreachable_trap(&self) -> ! {
        #[cfg(target_arch = "wasm32")]
        {
            unsafe { core::arch::wasm32::unreachable() }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            panic!("unreachable")
        }
    }

    pub fn emit_revert(&self) -> Result<(), CustomError> {
        // host::casper_return(ReturnFlags::REVERT, Some(&[1, 2, 3]));
        host::revert(Err(CustomError::Bar))
    }
}

use casper_sdk::Contract;
use vm_common::flags::ReturnFlags;

#[casper(export)]
pub fn call() {
    log!("calling create");
    match Greeter::create() {
        Ok(CreateResult {
            package_address,
            contract_address,
            version,
        }) => {
            log!("success");
            log!("package_address: {:?}", package_address);
            log!("contract_address: {:?}", contract_address);
            log!("version: {:?}", version);

            let call_0 = "get_greeting";
            let (maybe_data_0, result_code_0) =
                host::casper_call(&contract_address, 0, call_0, &[]);
            log!("{call_0:?} result={result_code_0:?}");
            assert_eq!(
                borsh::from_slice::<String>(&maybe_data_0.as_ref().expect("return value")).unwrap(),
                "".to_string()
            ); // TODO: Constructors

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

            let call_4: &str = "emit_revert";
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
        }
        Err(error) => {
            log!("error {:?}", error);
        }
    }

    log!("👋 Goodbye");
}

#[cfg(test)]
mod tests {

    use casper_sdk::{schema_helper, Contract};

    use super::*;

    #[test]
    fn test() {
        let args = ("hello".to_string(), 123);
        schema_helper::dispatch("call", &borsh::to_vec(&args).unwrap());
    }

    #[test]
    fn exports() {
        dbg!(schema_helper::list_exports());
    }

    #[test]
    fn compile_time_schema() {
        let schema = Greeter::schema();
        dbg!(&schema);
        assert_eq!(schema.name, "Greeter");
        assert_eq!(schema.entry_points[0].name, "get_greeting");
        // assert_eq!(schema.entry_points[0].name, "flip");
        assert_eq!(schema.entry_points[1].name, "set_greeting");
        // let s = serde_json::to_string_pretty(&schema).expect("foo");
        // println!("{s}");
    }

    #[test]
    fn should_greet() {
        assert_eq!(Greeter::name(), "Greeter");
        let mut flipper = Greeter::new();
        assert_eq!(flipper.get_greeting(), ""); // TODO: Initializer
        flipper.set_greeting("Hi".into());
        assert_eq!(flipper.get_greeting(), "Hi");
    }
}
