#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_contract_sdk::{
    compat::types::{CLType, CLValue, RuntimeArgs},
    prelude::*,
    serializers::Convention,
};

/// This contract implements a simple flipper.
#[derive(PanicOnDefault)]
#[casper(contract_state, abi_convention = Convention::Named)]
pub struct Contract;

#[casper]
impl Contract {
    pub fn add_with_default_abi_convention(a: u32, b: u32) -> u32 {
        a + b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_contract_sdk::{
        casper::native::{self, Environment, NativeTrap},
        casper_executor_wasm_common::flags::ReturnFlags,
        serializers::{borsh, AbiConvention},
    };

    #[test]
    #[should_panic(
        expected = "Failed to convert named argument \"a\": Expected U32 but found String"
    )]
    fn passing_incorrect_types_into_named_args_convention() {
        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert("a", "Hello ".to_string()).unwrap();
        runtime_args.insert("b", "world ".to_string()).unwrap();

        let env = Environment::default().with_input_data(borsh::to_vec(&runtime_args).unwrap());

        assert_eq!(Contract::DEFAULT_ABI_CONVENTION, Convention::Named);

        // This should panic with the expected message
        native::dispatch_with(env, || {
            native::invoke_export_by_name("add_with_default_abi_convention")
        })
        .unwrap_err();
    }

    #[test]
    fn test_calls_and_returns_named_arguments() {
        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert("a", 123u32).unwrap();
        runtime_args.insert("b", 456u32).unwrap();

        let env = Environment::default().with_input_data(borsh::to_vec(&runtime_args).unwrap());

        assert_eq!(Contract::DEFAULT_ABI_CONVENTION, Convention::Named);

        let NativeTrap::Return(_return_flags, return_bytes) = native::dispatch_with(env, || {
            native::invoke_export_by_name("add_with_default_abi_convention")
        })
        .unwrap_err() else {
            panic!("expected ret")
        };

        let ret_clvalue: CLValue =
            borsh::from_slice(&return_bytes).expect("Failed to deserialize return value");
        assert_eq!(ret_clvalue.cl_type(), &CLType::U32);
        assert_eq!(ret_clvalue.to_t::<u32>().unwrap(), 579);
    }
}
