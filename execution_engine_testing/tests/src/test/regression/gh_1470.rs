use casper_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    AccountHash, DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};

use casper_execution_engine::{
    core::{engine_state::Error, execution},
    shared::TypeMismatch,
};
use casper_types::{
    runtime_args, system::mint, CLTyped, ContractHash, ContractPackageHash, Key, RuntimeArgs,
};

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const GH_1470_REGRESSION: &str = "gh_1470_regression.wasm";
const GH_1470_REGRESSION_CALL: &str = "gh_1470_regression_call.wasm";

fn setup() -> InMemoryWasmTestBuilder {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let transfer = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => ACCOUNT_1_ADDR,
            mint::ARG_AMOUNT => MINIMUM_ACCOUNT_CREATION_BALANCE,
            mint::ARG_ID => Some(42u64),
        },
    )
    .build();

    builder.exec(transfer).expect_success().commit();

    builder
}

#[ignore]
#[test]
fn gh_1470_call_contract_should_verify_group_access() {
    let mut builder = setup();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_1470_REGRESSION,
        RuntimeArgs::new(),
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    let account_stored_value = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .unwrap();
    let account = account_stored_value.as_account().cloned().unwrap();

    let contract_hash_key = account
        .named_keys()
        .get(gh_1470_regression::CONTRACT_HASH_NAME)
        .cloned()
        .unwrap();
    let contract_hash = contract_hash_key
        .into_hash()
        .map(ContractHash::new)
        .unwrap();
    let contract_package_hash_key = account
        .named_keys()
        .get(gh_1470_regression::CONTRACT_PACKAGE_HASH_NAME)
        .cloned()
        .unwrap();
    let contract_package_hash = contract_package_hash_key
        .into_hash()
        .map(ContractPackageHash::new)
        .unwrap();

    let call_contract_request = {
        let args = runtime_args! {
            gh_1470_regression_call::ARG_TEST_METHOD => gh_1470_regression_call::METHOD_CALL_DO_NOTHING,
            gh_1470_regression_call::ARG_CONTRACT_HASH => contract_hash,
        };
        ExecuteRequestBuilder::standard(ACCOUNT_1_ADDR, GH_1470_REGRESSION_CALL, args).build()
    };

    builder.exec(call_contract_request).commit();

    let response = builder
        .get_exec_results()
        .last()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let call_contract_error = exec_response
        .as_error()
        .cloned()
        .expect("should have error");

    let call_versioned_contract_request = {
        let args = runtime_args! {
            gh_1470_regression_call::ARG_TEST_METHOD => gh_1470_regression_call::METHOD_CALL_VERSIONED_DO_NOTHING,
            gh_1470_regression_call::ARG_CONTRACT_PACKAGE_HASH => contract_package_hash,
        };
        ExecuteRequestBuilder::standard(ACCOUNT_1_ADDR, GH_1470_REGRESSION_CALL, args).build()
    };

    builder.exec(call_versioned_contract_request).commit();

    let response = builder
        .get_exec_results()
        .last()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let call_versioned_contract_error = exec_response.as_error().expect("should have error");

    match (&call_contract_error, &call_versioned_contract_error) {
        (
            Error::Exec(execution::Error::InvalidContext),
            Error::Exec(execution::Error::InvalidContext),
        ) => (),
        _ => panic!("Both variants should raise same error."),
    }

    assert!(matches!(
        call_versioned_contract_error,
        Error::Exec(execution::Error::InvalidContext)
    ));
    assert!(matches!(
        call_contract_error,
        Error::Exec(execution::Error::InvalidContext)
    ));
}

#[ignore]
#[test]
fn gh_1470_call_contract_should_verify_invalid_arguments_length() {
    let mut builder = setup();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_1470_REGRESSION,
        RuntimeArgs::new(),
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    let account_stored_value = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .unwrap();
    let account = account_stored_value.as_account().cloned().unwrap();

    let contract_hash_key = account
        .named_keys()
        .get(gh_1470_regression::CONTRACT_HASH_NAME)
        .cloned()
        .unwrap();
    let contract_hash = contract_hash_key
        .into_hash()
        .map(ContractHash::new)
        .unwrap();
    let contract_package_hash_key = account
        .named_keys()
        .get(gh_1470_regression::CONTRACT_PACKAGE_HASH_NAME)
        .cloned()
        .unwrap();
    let contract_package_hash = contract_package_hash_key
        .into_hash()
        .map(ContractPackageHash::new)
        .unwrap();

    let call_contract_request = {
        let args = runtime_args! {
            gh_1470_regression_call::ARG_TEST_METHOD => gh_1470_regression_call::METHOD_CALL_DO_NOTHING_NO_ARGS,
            gh_1470_regression_call::ARG_CONTRACT_HASH => contract_hash,
        };
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, GH_1470_REGRESSION_CALL, args)
            .build()
    };

    builder.exec(call_contract_request).commit();

    let response = builder
        .get_exec_results()
        .last()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let call_contract_error = exec_response
        .as_error()
        .cloned()
        .expect("should have error");

    let call_versioned_contract_request = {
        let args = runtime_args! {
            gh_1470_regression_call::ARG_TEST_METHOD => gh_1470_regression_call::METHOD_CALL_VERSIONED_DO_NOTHING_NO_ARGS,
            gh_1470_regression_call::ARG_CONTRACT_PACKAGE_HASH => contract_package_hash,
        };
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, GH_1470_REGRESSION_CALL, args)
            .build()
    };

    builder.exec(call_versioned_contract_request).commit();

    let response = builder
        .get_exec_results()
        .last()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let call_versioned_contract_error = exec_response.as_error().expect("should have error");

    match (&call_contract_error, &call_versioned_contract_error) {
        (
            Error::Exec(execution::Error::InvalidArgumentsLength {
                expected: 3,
                actual: 0,
            }),
            Error::Exec(execution::Error::InvalidArgumentsLength {
                expected: 3,
                actual: 0,
            }),
        ) => (),
        _ => panic!(
            "Both variants should raise same error: lhs={:?} rhs={:?}",
            call_contract_error, call_versioned_contract_error
        ),
    }

    assert!(matches!(
        call_versioned_contract_error,
        Error::Exec(execution::Error::InvalidArgumentsLength {
            expected: 3,
            actual: 0
        })
    ));
    assert!(matches!(
        call_contract_error,
        Error::Exec(execution::Error::InvalidArgumentsLength {
            expected: 3,
            actual: 0
        })
    ));
}

#[ignore]
#[test]
fn gh_1470_call_contract_should_verify_wrong_argument_types() {
    let mut builder = setup();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_1470_REGRESSION,
        RuntimeArgs::new(),
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    let account_stored_value = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .unwrap();
    let account = account_stored_value.as_account().cloned().unwrap();

    let contract_hash_key = account
        .named_keys()
        .get(gh_1470_regression::CONTRACT_HASH_NAME)
        .cloned()
        .unwrap();
    let contract_hash = contract_hash_key
        .into_hash()
        .map(ContractHash::new)
        .unwrap();
    let contract_package_hash_key = account
        .named_keys()
        .get(gh_1470_regression::CONTRACT_PACKAGE_HASH_NAME)
        .cloned()
        .unwrap();
    let contract_package_hash = contract_package_hash_key
        .into_hash()
        .map(ContractPackageHash::new)
        .unwrap();

    let call_contract_request = {
        let args = runtime_args! {
            gh_1470_regression_call::ARG_TEST_METHOD => gh_1470_regression_call::METHOD_CALL_DO_NOTHING_TYPE_MISMATCH,
            gh_1470_regression_call::ARG_CONTRACT_HASH => contract_hash,
        };
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, GH_1470_REGRESSION_CALL, args)
            .build()
    };

    builder.exec(call_contract_request).commit();

    let response = builder
        .get_exec_results()
        .last()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let call_contract_error = exec_response
        .as_error()
        .cloned()
        .expect("should have error");

    let call_versioned_contract_request = {
        let args = runtime_args! {
            gh_1470_regression_call::ARG_TEST_METHOD => gh_1470_regression_call::METHOD_CALL_VERSIONED_DO_NOTHING_TYPE_MISMATCH,
            gh_1470_regression_call::ARG_CONTRACT_PACKAGE_HASH => contract_package_hash,
        };
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, GH_1470_REGRESSION_CALL, args)
            .build()
    };

    builder.exec(call_versioned_contract_request).commit();

    let response = builder
        .get_exec_results()
        .last()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let call_versioned_contract_error = exec_response.as_error().expect("should have error");

    let expected = gh_1470_regression::Arg1Type::cl_type();
    let found = gh_1470_regression::Arg3Type::cl_type();

    let expected_type_mismatch =
        TypeMismatch::new(format!("{:?}", expected), format!("{:?}", found));

    match (&call_contract_error, &call_versioned_contract_error) {
        (
            Error::Exec(execution::Error::TypeMismatch(lhs_type_mismatch)),
            Error::Exec(execution::Error::TypeMismatch(rhs_type_mismatch)),
        ) if lhs_type_mismatch == &expected_type_mismatch
            && rhs_type_mismatch == &expected_type_mismatch => {}
        _ => panic!(
            "Both variants should raise same error: lhs={:?} rhs={:?}",
            call_contract_error, call_versioned_contract_error
        ),
    }

    assert!(matches!(
        call_versioned_contract_error,
        Error::Exec(execution::Error::TypeMismatch(type_mismatch)) if type_mismatch == &expected_type_mismatch
    ));
    assert!(matches!(
        call_contract_error,
        Error::Exec(execution::Error::TypeMismatch(type_mismatch)) if type_mismatch == expected_type_mismatch
    ));
}
