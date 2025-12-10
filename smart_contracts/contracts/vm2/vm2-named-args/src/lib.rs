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

    // Explicit Positional ABI with unit return
    #[casper(abi_convention = AbiConvention::Positional)]
    pub fn positional_unit_no_args() {}
}

#[casper]
impl ContractTrait for Contract {}

// Default (Positional) export with a value return.
#[casper(export)]
pub fn positional_export_inc(x: u32) -> u32 {
    x + 1
}

// Default (Positional) export with unit return.
#[casper(export)]
pub fn positional_export_no_args_unit() {}

// Named export
#[casper(export, abi_convention = AbiConvention::Named)]
pub fn named_export_add(a: u32, b: u32) -> u32 {
    a + b
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use casper_contract_sdk::{
        casper::native::{run_expecting_panic, with_env, EnvironmentMock, ExpectedCall},
        common::error::HOST_ERROR_SUCCESS,
        compat::types::{CLTyped, CLValue, RuntimeArgs},
        serializers::{borsh, AbiConfig},
        sys::EnvInfo,
    };

    #[test]
    fn passing_incorrect_types_into_named_args_convention() {
        assert_eq!(Contract::DEFAULT_ABI_CONVENTION, AbiConvention::Named);
        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert("a", "Hello ".to_string()).unwrap();
        runtime_args.insert("b", "world ".to_string()).unwrap();
        let input_bytes = borsh::to_vec(&runtime_args).expect("expected args to serialize");
        let env = Arc::new(EnvironmentMock::new());
        env.add_expectation(ExpectedCall::expect_copy_input(&input_bytes));
        with_env(env.clone(), || {
            let res = run_expecting_panic(|| {
                __casper_export_args_with_default_abi_convention();
            });
            assert!(res.is_err());
            let maybe_err_msg = res
                .err()
                .and_then(|trap| trap.downcast_value::<String>().map(|x| x.clone()));
            assert_eq!(
                maybe_err_msg,
                Some(
                    "Failed to convert named argument \"a\": Expected U32 but found String."
                        .to_string()
                )
            );
        });
    }

    #[test]
    fn test_calls_and_returns_named_arguments() {
        assert_eq!(Contract::DEFAULT_ABI_CONVENTION, AbiConvention::Named);
        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert("a", 123u32).unwrap();
        runtime_args.insert("b", 456u32).unwrap();
        let input_bytes = borsh::to_vec(&runtime_args).expect("expected args to serialize");
        let env = Arc::new(EnvironmentMock::new());
        env.add_expectation(ExpectedCall::expect_copy_input(&input_bytes));
        env.add_expectation(ExpectedCall::expect_get_info(Some(EnvInfo::default())));

        let expected_returned_data = vec![123u32, 456u32];
        let ret_clvalue = CLValue::from_t(&expected_returned_data)
            .expect("Failed to convert return value to CLValue");
        let expected_return_data =
            borsh::to_vec(&ret_clvalue).expect("Expected borsh to serialize data");
        env.add_expectation(ExpectedCall::expect_return(
            Some((0, Some(expected_return_data))),
            HOST_ERROR_SUCCESS,
        ));
        with_env(env.clone(), || {
            // The panic is from the casper_ret sdk function
            let _ = run_expecting_panic(|| __casper_export_args_with_default_abi_convention());
        });
    }

    #[test]
    fn test_named_convention_with_unit_ret() {
        assert_eq!(Contract::DEFAULT_ABI_CONVENTION, AbiConvention::Named);
        let runtime_args = RuntimeArgs::new();
        let input_bytes = borsh::to_vec(&runtime_args).expect("expected args to serialize");
        let env = Arc::new(EnvironmentMock::new());
        env.add_expectation(ExpectedCall::expect_copy_input(&input_bytes));
        env.add_expectation(ExpectedCall::expect_get_info(Some(EnvInfo::default())));
        let ret_clvalue = CLValue::UNIT;
        let expected_return_data =
            borsh::to_vec(&ret_clvalue).expect("Expected borsh to serialize data");
        env.add_expectation(ExpectedCall::expect_return(
            Some((0, Some(expected_return_data))),
            HOST_ERROR_SUCCESS,
        ));
        with_env(env.clone(), || {
            // The panic is from the casper_ret sdk function
            let _ = run_expecting_panic(|| {
                __casper_export_unit_ret_value_with_default_abi_convention()
            });
        });
    }

    #[test]
    fn test_calls_overriden_convnention() {
        let args = (123u32, 456u32, 789u32);
        let input_bytes = borsh::to_vec(&args).expect("expected args to serialize");
        let env = Arc::new(EnvironmentMock::new());
        env.add_expectation(ExpectedCall::expect_copy_input(&input_bytes));
        env.add_expectation(ExpectedCall::expect_get_info(Some(EnvInfo::default())));

        let expected_return_data =
            borsh::to_vec(&vec![123u32, 456u32, 789u32]).expect("Expected borsh to serialize data");
        env.add_expectation(ExpectedCall::expect_return(
            Some((0, Some(expected_return_data))),
            HOST_ERROR_SUCCESS,
        ));
        with_env(env.clone(), || {
            // The panic is from the casper_ret sdk function
            let _ =
                run_expecting_panic(|| __casper_export_inner_args_with_overriden_abi_convention());
        });
    }

    #[test]
    fn trait_method_has_default_convention() {
        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert("a", 123u32).unwrap();
        runtime_args.insert("b", 456u32).unwrap();
        let input_bytes = borsh::to_vec(&runtime_args).expect("expected args to serialize");

        let env = Arc::new(EnvironmentMock::new());
        env.add_expectation(ExpectedCall::expect_copy_input(&input_bytes));

        let expected_returned_data = vec![123u32, 456u32];
        let ret_clvalue = CLValue::from_t(&expected_returned_data)
            .expect("Failed to convert return value to CLValue");
        let expected_return_data =
            borsh::to_vec(&ret_clvalue).expect("Expected borsh to serialize data");
        env.add_expectation(ExpectedCall::expect_return(
            Some((0, Some(expected_return_data))),
            HOST_ERROR_SUCCESS,
        ));
        with_env(env.clone(), || {
            let _ = run_expecting_panic(|| trait_args_with_default_abi_convention::<Contract>());
        });
    }

    #[test]
    fn trait_method_has_different_convention() {
        let args = (123u32, 456u32, 789u32);
        let input_bytes = borsh::to_vec(&args).expect("expected args to serialize");
        let env = Arc::new(EnvironmentMock::new());
        env.add_expectation(ExpectedCall::expect_copy_input(&input_bytes));
        let expected_return_data =
            borsh::to_vec(&vec![123u32, 456u32, 789u32]).expect("Expected borsh to serialize data");
        env.add_expectation(ExpectedCall::expect_return(
            Some((0, Some(expected_return_data))),
            HOST_ERROR_SUCCESS,
        ));
        with_env(env.clone(), || {
            // The panic is from the casper_ret sdk function
            let _ = run_expecting_panic(|| trait_args_with_different_abi_convention::<Contract>());
        });
    }

    #[test]
    fn foobar() {
        let abi_items = casper_contract_sdk::abi::collector::ABI_ITEMS
            .iter()
            .collect::<Vec<_>>();

        let smart_contract = abi_items
            .iter()
            .find_map(|item| item.as_smart_contract())
            .expect("Expected smart contract item");
        assert_eq!(smart_contract.abi_convention, AbiConvention::Named);

        let ctor = abi_items
            .iter()
            .filter_map(|item| item.as_entry_point())
            .find(|e| e.name == "new")
            .expect("Expected entry point");

        assert!(ctor.is_constructor);

        let e1 = abi_items
            .iter()
            .filter_map(|item| item.as_entry_point())
            .find(|e| e.name == "args_with_overriden_abi_convention")
            .expect("Expected entry point");
        assert_eq!(e1.abi_convention, AbiConvention::Positional);
        assert_eq!((e1.result_decl.type_name)(), "alloc::vec::Vec<u32>");
        assert_eq!(e1.result_decl.cl_type(), Vec::<u32>::cl_type());
        assert_eq!(e1.params.len(), 3);
        assert_eq!(e1.params[0].name, "a");
        assert_eq!((e1.params[0].decl.type_name)(), "u32");
        assert_eq!(e1.params[0].decl.cl_type(), u32::cl_type());
        assert_eq!(e1.params[1].name, "b");
        assert_eq!((e1.params[1].decl.type_name)(), "u32");
        assert_eq!(e1.params[1].decl.cl_type(), u32::cl_type());
        assert_eq!(e1.params[2].name, "c");
        assert_eq!((e1.params[2].decl.type_name)(), "u32");
        assert_eq!(e1.params[2].decl.cl_type(), u32::cl_type());

        let e2 = abi_items
            .iter()
            .filter_map(|item| item.as_entry_point())
            .find(|e| e.export_name == "args_with_default_abi_convention")
            .expect("Expected entry point");
        assert_eq!(e2.abi_convention, AbiConvention::Named);
    }

    #[test]
    fn test_positional_impl_unit_ret_calls_ret_none() {
        let env = Arc::new(EnvironmentMock::new());
        env.add_expectation(ExpectedCall::expect_copy_input(&[]));
        env.add_expectation(ExpectedCall::expect_get_info(Some(EnvInfo::default())));
        env.add_expectation(ExpectedCall::expect_return(
            Some((0, None)),
            HOST_ERROR_SUCCESS,
        ));
        with_env(env.clone(), || {
            let _ = run_expecting_panic(|| __casper_export_positional_unit_no_args());
        });
    }

    #[test]
    fn test_positional_export_value_ret() {
        let env = Arc::new(EnvironmentMock::new());

        let input_bytes = borsh::to_vec(&(7u32,)).expect("borsh");
        env.add_expectation(ExpectedCall::expect_copy_input(&input_bytes));

        let expected_ret = borsh::to_vec(&8u32).expect("borsh");
        env.add_expectation(ExpectedCall::expect_return(
            Some((0, Some(expected_ret))),
            HOST_ERROR_SUCCESS,
        ));
        with_env(env.clone(), || {
            let _ = run_expecting_panic(|| __casper_export_positional_export_inc());
        });
    }

    #[test]
    fn test_named_export_value_ret() {
        let env = Arc::new(EnvironmentMock::new());
        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert("a", 2u32).unwrap();
        runtime_args.insert("b", 3u32).unwrap();
        let input_bytes = borsh::to_vec(&runtime_args).expect("borsh");
        env.add_expectation(ExpectedCall::expect_copy_input(&input_bytes));
        // Named returns CLValue-encoded value
        let ret_clvalue = CLValue::from_t(&5u32).expect("clvalue");
        let expected_ret = borsh::to_vec(&ret_clvalue).expect("borsh");
        env.add_expectation(ExpectedCall::expect_return(
            Some((0, Some(expected_ret))),
            HOST_ERROR_SUCCESS,
        ));
        with_env(env.clone(), || {
            let _ = run_expecting_panic(|| __casper_export_named_export_add());
        });
    }

    #[test]
    fn test_positional_export_unit_ret_calls_ret_none() {
        let env = Arc::new(EnvironmentMock::new());
        env.add_expectation(ExpectedCall::expect_copy_input(&[]));
        env.add_expectation(ExpectedCall::expect_return(
            Some((0, None)),
            HOST_ERROR_SUCCESS,
        ));
        with_env(env.clone(), || {
            let _ = run_expecting_panic(|| __casper_export_positional_export_no_args_unit());
        });
    }
}
