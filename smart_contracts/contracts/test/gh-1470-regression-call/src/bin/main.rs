#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use core::str::FromStr;
use gh_1470_regression_call::{ARG_CONTRACT_HASH, ARG_CONTRACT_PACKAGE_HASH, ARG_TEST_METHOD};

use casper_contract::{contract_api::runtime, unwrap_or_revert::UnwrapOrRevert};
use casper_types::{runtime_args, ContractHash, ContractPackageHash, RuntimeArgs};

use gh_1470_regression_call::TestMethod;

#[no_mangle]
pub extern "C" fn call() {
    let test_method = {
        let arg_test_method: String = runtime::get_named_arg(ARG_TEST_METHOD);
        TestMethod::from_str(&arg_test_method).unwrap_or_revert()
    };

    let correct_runtime_args = runtime_args! {
        gh_1470_regression::ARG3 => gh_1470_regression::Arg3Type::default(),
        gh_1470_regression::ARG2 => gh_1470_regression::Arg2Type::default(),
        gh_1470_regression::ARG1 => gh_1470_regression::Arg1Type::default(),
    };

    let no_runtime_args = runtime_args! {};

    let type_mismatch_runtime_args = runtime_args! {
        gh_1470_regression::ARG2 => gh_1470_regression::Arg1Type::default(),
        gh_1470_regression::ARG3 => gh_1470_regression::Arg2Type::default(),
        gh_1470_regression::ARG1 => gh_1470_regression::Arg3Type::default(),
    };

    let optional_type_mismatch_runtime_args = runtime_args! {
        gh_1470_regression::ARG1 => gh_1470_regression::Arg1Type::default(),
        gh_1470_regression::ARG2 => gh_1470_regression::Arg2Type::default(),
        gh_1470_regression::ARG3 => gh_1470_regression::Arg4Type::default(),
    };

    let correct_without_optional_args = runtime_args! {
        gh_1470_regression::ARG2 => gh_1470_regression::Arg2Type::default(),
        gh_1470_regression::ARG1 => gh_1470_regression::Arg1Type::default(),
    };

    let extra_runtime_args = runtime_args! {
        gh_1470_regression::ARG3 => gh_1470_regression::Arg3Type::default(),
        gh_1470_regression::ARG2 => gh_1470_regression::Arg2Type::default(),
        gh_1470_regression::ARG1 => gh_1470_regression::Arg1Type::default(),
        gh_1470_regression::ARG4 => gh_1470_regression::Arg4Type::default(),
        gh_1470_regression::ARG5 => gh_1470_regression::Arg5Type::default(),
    };

    assert_ne!(correct_runtime_args, optional_type_mismatch_runtime_args);

    match test_method {
        TestMethod::CallDoNothing => {
            let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);

            runtime::call_contract::<()>(
                contract_hash,
                gh_1470_regression::RESTRICTED_DO_NOTHING_ENTRYPOINT,
                correct_runtime_args,
            );
        }
        TestMethod::CallVersionedDoNothing => {
            let contract_package_hash: ContractPackageHash =
                runtime::get_named_arg(ARG_CONTRACT_PACKAGE_HASH);

            runtime::call_versioned_contract::<()>(
                contract_package_hash,
                None,
                gh_1470_regression::RESTRICTED_DO_NOTHING_ENTRYPOINT,
                correct_runtime_args,
            );
        }
        TestMethod::CallDoNothingNoArgs => {
            let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);

            runtime::call_contract::<()>(
                contract_hash,
                gh_1470_regression::RESTRICTED_DO_NOTHING_ENTRYPOINT,
                no_runtime_args,
            );
        }
        TestMethod::CallVersionedDoNothingNoArgs => {
            let contract_package_hash: ContractPackageHash =
                runtime::get_named_arg(ARG_CONTRACT_PACKAGE_HASH);

            runtime::call_versioned_contract::<()>(
                contract_package_hash,
                None,
                gh_1470_regression::RESTRICTED_DO_NOTHING_ENTRYPOINT,
                no_runtime_args,
            );
        }
        TestMethod::CallDoNothingTypeMismatch => {
            let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);

            runtime::call_contract::<()>(
                contract_hash,
                gh_1470_regression::RESTRICTED_DO_NOTHING_ENTRYPOINT,
                type_mismatch_runtime_args,
            );
        }

        TestMethod::CallVersionedDoNothingTypeMismatch => {
            let contract_package_hash: ContractPackageHash =
                runtime::get_named_arg(ARG_CONTRACT_PACKAGE_HASH);

            runtime::call_versioned_contract::<()>(
                contract_package_hash,
                None,
                gh_1470_regression::RESTRICTED_DO_NOTHING_ENTRYPOINT,
                type_mismatch_runtime_args,
            );
        }
        TestMethod::CallDoNothingNoOptionals => {
            let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);

            runtime::call_contract::<()>(
                contract_hash,
                gh_1470_regression::RESTRICTED_DO_NOTHING_ENTRYPOINT,
                correct_without_optional_args,
            );
        }
        TestMethod::CallVersionedDoNothingNoOptionals => {
            let contract_package_hash: ContractPackageHash =
                runtime::get_named_arg(ARG_CONTRACT_PACKAGE_HASH);

            runtime::call_versioned_contract::<()>(
                contract_package_hash,
                None,
                gh_1470_regression::RESTRICTED_DO_NOTHING_ENTRYPOINT,
                correct_without_optional_args,
            );
        }
        TestMethod::CallDoNothingExtra => {
            let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);

            runtime::call_contract::<()>(
                contract_hash,
                gh_1470_regression::RESTRICTED_WITH_EXTRA_ARG_ENTRYPOINT,
                extra_runtime_args,
            );
        }
        TestMethod::CallVersionedDoNothingExtra => {
            let contract_package_hash: ContractPackageHash =
                runtime::get_named_arg(ARG_CONTRACT_PACKAGE_HASH);

            runtime::call_versioned_contract::<()>(
                contract_package_hash,
                None,
                gh_1470_regression::RESTRICTED_WITH_EXTRA_ARG_ENTRYPOINT,
                extra_runtime_args,
            );
        }
        TestMethod::CallDoNothingOptionalTypeMismatch => {
            let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);

            runtime::call_contract::<()>(
                contract_hash,
                gh_1470_regression::RESTRICTED_DO_NOTHING_ENTRYPOINT,
                optional_type_mismatch_runtime_args,
            );
        }
        TestMethod::CallVersionedDoNothingOptionalTypeMismatch => {
            let contract_package_hash: ContractPackageHash =
                runtime::get_named_arg(ARG_CONTRACT_PACKAGE_HASH);

            runtime::call_versioned_contract::<()>(
                contract_package_hash,
                None,
                gh_1470_regression::RESTRICTED_DO_NOTHING_ENTRYPOINT,
                optional_type_mismatch_runtime_args,
            );
        }
    }
}
