#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_contract_sdk::{prelude::*, serializers::AbiConvention};

#[casper(abi_convention = AbiConvention::Named)]
pub trait ContractTrait {
    fn trait_args_with_default_abi_convention(a: u32, b: u32) -> Vec<u32> {
        vec![a, b]
    }
    #[casper(abi_convention = AbiConvention::Positional)]
    fn trait_args_with_different_abi_convention(a: u32, b: u32, c: u32) -> Vec<u32> {
        vec![a, b, c]
    }
}

/// This contract implements a simple flipper.
#[derive(Default)]
#[casper(contract_state, abi_convention = AbiConvention::Named)]
pub struct Contract {
    pub value: u32,
}

#[casper]
impl Contract {
    #[casper(constructor)]
    pub fn new(value: u32) -> Self {
        Self { value }
    }
    pub fn args_with_default_abi_convention(a: u32, b: u32) -> Vec<u32> {
        vec![a, b]
    }

    pub fn unit_ret_value_with_default_abi_convention() {
        // This function returns a unit value, which is compatible with the default ABI convention.
    }

    #[casper(abi_convention = AbiConvention::Positional)]
    pub fn args_with_overriden_abi_convention(a: u32, b: u32, c: u32) -> Vec<u32> {
        vec![a, b, c]
    }
}

#[casper]
impl ContractTrait for Contract {}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_contract_sdk::{
        casper::native::{self, Environment, NativeTrap},
        compat::types::{CLTyped, CLValue, RuntimeArgs},
        serializers::{borsh, AbiConfig},
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

        assert_eq!(Contract::DEFAULT_ABI_CONVENTION, AbiConvention::Named);

        // This should panic with the expected message
        native::dispatch_with(env, || {
            native::invoke_export_by_name("args_with_default_abi_convention")
        })
        .unwrap_err();
    }

    #[test]
    fn test_calls_and_returns_named_arguments() {
        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert("a", 123u32).unwrap();
        runtime_args.insert("b", 456u32).unwrap();

        let env = Environment::default().with_input_data(borsh::to_vec(&runtime_args).unwrap());

        assert_eq!(Contract::DEFAULT_ABI_CONVENTION, AbiConvention::Named);

        let NativeTrap::Return(_return_flags, return_bytes) = native::dispatch_with(env, || {
            native::invoke_export_by_name("args_with_default_abi_convention")
        })
        .unwrap_err() else {
            panic!("expected ret")
        };

        let ret_clvalue: CLValue =
            borsh::from_slice(&return_bytes).expect("Failed to deserialize return value");
        assert_eq!(ret_clvalue.cl_type(), &Vec::<u32>::cl_type());
        assert_eq!(ret_clvalue.to_t::<Vec<u32>>().unwrap(), vec![123, 456]);
    }

    #[test]
    fn test_named_convention_with_unit_ret() {
        let runtime_args = RuntimeArgs::new();
        let env = Environment::default().with_input_data(borsh::to_vec(&runtime_args).unwrap());

        assert_eq!(Contract::DEFAULT_ABI_CONVENTION, AbiConvention::Named);

        let NativeTrap::Return(_return_flags, return_bytes) = native::dispatch_with(env, || {
            native::invoke_export_by_name("unit_ret_value_with_default_abi_convention")
        })
        .unwrap_err() else {
            panic!("expected ret")
        };

        let ret_clvalue: CLValue =
            borsh::from_slice(&return_bytes).expect("Failed to deserialize return value");
        assert_eq!(ret_clvalue, CLValue::UNIT);
    }

    #[test]
    fn test_calls_overriden_convnention() {
        let args = (123u32, 456u32, 789u32);

        let env = Environment::default().with_input_data(borsh::to_vec(&args).unwrap());

        assert_eq!(Contract::DEFAULT_ABI_CONVENTION, AbiConvention::Named);

        let NativeTrap::Return(_return_flags, return_bytes) = native::dispatch_with(env, || {
            native::invoke_export_by_name("args_with_overriden_abi_convention")
        })
        .unwrap_err() else {
            panic!("expected ret")
        };

        let ret_value: Vec<u32> =
            borsh::from_slice(&return_bytes).expect("Failed to deserialize return value");
        assert_eq!(ret_value, vec![123, 456, 789]);
    }

    #[test]
    fn trait_method_has_default_convention() {
        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert("a", 123u32).unwrap();
        runtime_args.insert("b", 456u32).unwrap();

        let env = Environment::default().with_input_data(borsh::to_vec(&runtime_args).unwrap());

        assert_eq!(Contract::DEFAULT_ABI_CONVENTION, AbiConvention::Named);
        let NativeTrap::Return(_return_flags, return_bytes) = native::dispatch_with(env, || {
            native::invoke_export_by_name("ContractTrait_trait_args_with_default_abi_convention")
        })
        .unwrap_err() else {
            panic!("expected ret")
        };
        let ret_clvalue: CLValue =
            borsh::from_slice(&return_bytes).expect("Failed to deserialize return value");
        assert_eq!(ret_clvalue.cl_type(), &<Vec<u32>>::cl_type());
        assert_eq!(ret_clvalue.to_t::<Vec<u32>>().unwrap(), vec![123, 456]);
    }

    #[test]
    fn trait_method_has_different_convention() {
        let arguments = (123u32, 456u32, 789u32);

        let env = Environment::default().with_input_data(borsh::to_vec(&arguments).unwrap());

        assert_eq!(Contract::DEFAULT_ABI_CONVENTION, AbiConvention::Named);
        let NativeTrap::Return(_return_flags, return_bytes) = native::dispatch_with(env, || {
            native::invoke_export_by_name("ContractTrait_trait_args_with_different_abi_convention")
        })
        .unwrap_err() else {
            panic!("expected ret")
        };
        let result: Vec<u32> =
            borsh::from_slice(&return_bytes).expect("Failed to deserialize return value");
        assert_eq!(result, vec![123, 456, 789]);
    }
}
