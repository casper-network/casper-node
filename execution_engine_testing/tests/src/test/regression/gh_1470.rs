use casper_engine_test_support::{
    internal::{
        ExecuteRequestBuilder, InMemoryWasmTestBuilder, UpgradeRequestBuilder,
        DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_RUN_GENESIS_REQUEST,
    },
    AccountHash, DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};

use casper_execution_engine::{
    core::{engine_state::Error, execution},
    shared::TypeMismatch,
};
use casper_types::{
    runtime_args,
    system::{
        auction::{self, DelegationRate},
        mint,
    },
    CLTyped, ContractHash, ContractPackageHash, EraId, Key, ProtocolVersion, RuntimeArgs, U512,
};

use crate::lmdb_fixture;

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const GH_1470_REGRESSION: &str = "gh_1470_regression.wasm";
const GH_1470_REGRESSION_CALL: &str = "gh_1470_regression_call.wasm";
const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);

const CONTRACT_ADD_BID: &str = "add_bid.wasm";
const BOND_AMOUNT: u64 = 42;
const BID_DELEGATION_RATE: DelegationRate = auction::DELEGATION_RATE_DENOMINATOR;

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";

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
            Error::Exec(execution::Error::MissingArgument { name: lhs_name }),
            Error::Exec(execution::Error::MissingArgument { name: rhs_name }),
        ) if lhs_name == rhs_name => (),
        _ => panic!(
            "Both variants should raise same error: lhs={:?} rhs={:?}",
            call_contract_error, call_versioned_contract_error
        ),
    }

    assert!(
        matches!(
            &call_versioned_contract_error,
            Error::Exec(execution::Error::MissingArgument {
                name,
            })
            if name == gh_1470_regression::ARG1
        ),
        "{:?}",
        call_versioned_contract_error
    );
    assert!(
        matches!(
            &call_contract_error,
            Error::Exec(execution::Error::MissingArgument {
                name,
            })
            if name == gh_1470_regression::ARG1
        ),
        "{:?}",
        call_contract_error
    );
}

#[ignore]
#[test]
fn gh_1470_call_contract_should_ignore_optional_args() {
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
            gh_1470_regression_call::ARG_TEST_METHOD => gh_1470_regression_call::METHOD_CALL_DO_NOTHING_NO_OPTIONALS,
            gh_1470_regression_call::ARG_CONTRACT_HASH => contract_hash,
        };
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, GH_1470_REGRESSION_CALL, args)
            .build()
    };

    builder
        .exec(call_contract_request)
        .expect_success()
        .commit();

    let call_versioned_contract_request = {
        let args = runtime_args! {
            gh_1470_regression_call::ARG_TEST_METHOD => gh_1470_regression_call::METHOD_CALL_VERSIONED_DO_NOTHING_NO_OPTIONALS,
            gh_1470_regression_call::ARG_CONTRACT_PACKAGE_HASH => contract_package_hash,
        };
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, GH_1470_REGRESSION_CALL, args)
            .build()
    };

    builder
        .exec(call_versioned_contract_request)
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn gh_1470_call_contract_should_not_accept_extra_args() {
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
            gh_1470_regression_call::ARG_TEST_METHOD => gh_1470_regression_call::METHOD_CALL_DO_NOTHING_EXTRA,
            gh_1470_regression_call::ARG_CONTRACT_HASH => contract_hash,
        };
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, GH_1470_REGRESSION_CALL, args)
            .build()
    };

    builder
        .exec(call_contract_request)
        .expect_success()
        .commit();

    let call_versioned_contract_request = {
        let args = runtime_args! {
            gh_1470_regression_call::ARG_TEST_METHOD => gh_1470_regression_call::METHOD_CALL_VERSIONED_DO_NOTHING_EXTRA,
            gh_1470_regression_call::ARG_CONTRACT_PACKAGE_HASH => contract_package_hash,
        };
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, GH_1470_REGRESSION_CALL, args)
            .build()
    };

    builder
        .exec(call_versioned_contract_request)
        .expect_success()
        .commit();
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

#[ignore]
#[test]
fn gh_1470_call_contract_should_verify_wrong_optional_argument_types() {
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
            gh_1470_regression_call::ARG_TEST_METHOD => gh_1470_regression_call::METHOD_CALL_DO_NOTHING_OPTIONAL_TYPE_MISMATCH,
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
            gh_1470_regression_call::ARG_TEST_METHOD => gh_1470_regression_call::METHOD_CALL_VERSIONED_DO_NOTHING_OPTIONAL_TYPE_MISMATCH,
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

    let expected = gh_1470_regression::Arg3Type::cl_type();
    let found = gh_1470_regression::Arg4Type::cl_type();

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

#[ignore]
#[test]
fn should_transfer_after_major_version_bump_from_1_2_0() {
    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_2_0);

    let transfer_args = runtime_args! {
        mint::ARG_AMOUNT => U512::one(),
        mint::ARG_TARGET => AccountHash::new([3; 32]),
        mint::ARG_ID => Some(1u64),
    };

    let current_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let old_protocol_data = builder
        .get_engine_state()
        .get_protocol_data(current_protocol_version)
        .expect("should have result")
        .expect("should have protocol data");

    let new_protocol_version =
        ProtocolVersion::from_parts(current_protocol_version.value().major + 1, 0, 0);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(current_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder
        .upgrade_with_upgrade_request(&mut upgrade_request)
        .expect_upgrade_success();

    let new_protocol_data = builder
        .get_engine_state()
        .get_protocol_data(new_protocol_version)
        .expect("should have result")
        .expect("should have protocol data");

    assert_eq!(new_protocol_data.mint(), old_protocol_data.mint());

    let transfer = ExecuteRequestBuilder::transfer(*DEFAULT_ACCOUNT_ADDR, transfer_args)
        .with_protocol_version(new_protocol_version)
        .build();
    builder.exec(transfer).expect_success().commit();
}

#[ignore]
#[test]
fn should_transfer_after_minor_version_bump_from_1_2_0() {
    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_2_0);

    let transfer_args = runtime_args! {
        mint::ARG_AMOUNT => U512::one(),
        mint::ARG_TARGET => AccountHash::new([3; 32]),
        mint::ARG_ID => Some(1u64),
    };

    let current_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let old_protocol_data = builder
        .get_engine_state()
        .get_protocol_data(current_protocol_version)
        .expect("should have result")
        .expect("should have protocol data");

    let new_protocol_version = ProtocolVersion::from_parts(
        current_protocol_version.value().major,
        current_protocol_version.value().minor + 1,
        0,
    );

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(current_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder
        .upgrade_with_upgrade_request(&mut upgrade_request)
        .expect_upgrade_success();

    let new_protocol_data = builder
        .get_engine_state()
        .get_protocol_data(new_protocol_version)
        .expect("should have result")
        .expect("should have protocol data");

    assert_eq!(new_protocol_data.mint(), old_protocol_data.mint());

    let transfer = ExecuteRequestBuilder::transfer(*DEFAULT_ACCOUNT_ADDR, transfer_args)
        .with_protocol_version(new_protocol_version)
        .build();
    builder.exec(transfer).expect_success().commit();
}

#[ignore]
#[test]
fn should_add_bid_after_major_bump() {
    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_2_0);

    let current_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let old_protocol_data = builder
        .get_engine_state()
        .get_protocol_data(current_protocol_version)
        .expect("should have result")
        .expect("should have protocol data");

    let new_protocol_version =
        ProtocolVersion::from_parts(current_protocol_version.value().major + 1, 0, 0);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(current_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder
        .upgrade_with_upgrade_request(&mut upgrade_request)
        .expect_upgrade_success();

    let new_protocol_data = builder
        .get_engine_state()
        .get_protocol_data(new_protocol_version)
        .expect("should have result")
        .expect("should have protocol data");

    assert_eq!(new_protocol_data.mint(), old_protocol_data.mint());

    let _default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let add_bid_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => U512::from(BOND_AMOUNT),
            auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
        },
    )
    .with_protocol_version(new_protocol_version)
    .build();

    builder.exec(add_bid_request).expect_success().commit();
}

#[ignore]
#[test]
fn should_add_bid_after_minor_bump() {
    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_2_0);

    let current_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let old_protocol_data = builder
        .get_engine_state()
        .get_protocol_data(current_protocol_version)
        .expect("should have result")
        .expect("should have protocol data");

    let new_protocol_version = ProtocolVersion::from_parts(
        current_protocol_version.value().major,
        current_protocol_version.value().minor + 1,
        0,
    );

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(current_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder
        .upgrade_with_upgrade_request(&mut upgrade_request)
        .expect_upgrade_success();

    let new_protocol_data = builder
        .get_engine_state()
        .get_protocol_data(new_protocol_version)
        .expect("should have result")
        .expect("should have protocol data");

    assert_eq!(new_protocol_data.mint(), old_protocol_data.mint());

    let _default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let add_bid_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => U512::from(BOND_AMOUNT),
            auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
        },
    )
    .with_protocol_version(new_protocol_version)
    .build();

    builder.exec(add_bid_request).expect_success().commit();
}

#[ignore]
#[test]
fn should_wasm_transfer_after_major_bump() {
    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_2_0);

    let current_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let old_protocol_data = builder
        .get_engine_state()
        .get_protocol_data(current_protocol_version)
        .expect("should have result")
        .expect("should have protocol data");

    let new_protocol_version =
        ProtocolVersion::from_parts(current_protocol_version.value().major + 1, 0, 0);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(current_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder
        .upgrade_with_upgrade_request(&mut upgrade_request)
        .expect_upgrade_success();

    let new_protocol_data = builder
        .get_engine_state()
        .get_protocol_data(new_protocol_version)
        .expect("should have result")
        .expect("should have protocol data");

    assert_eq!(new_protocol_data.mint(), old_protocol_data.mint());

    let _default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let wasm_transfer = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_AMOUNT => U512::one(),
            ARG_TARGET => AccountHash::new([1; 32]),
        },
    )
    .with_protocol_version(new_protocol_version)
    .build();

    builder.exec(wasm_transfer).expect_success().commit();
}

#[ignore]
#[test]
fn should_wasm_transfer_after_minor_bump() {
    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_2_0);

    let current_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let old_protocol_data = builder
        .get_engine_state()
        .get_protocol_data(current_protocol_version)
        .expect("should have result")
        .expect("should have protocol data");

    let new_protocol_version = ProtocolVersion::from_parts(
        current_protocol_version.value().major,
        current_protocol_version.value().minor + 1,
        0,
    );

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(current_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder
        .upgrade_with_upgrade_request(&mut upgrade_request)
        .expect_upgrade_success();

    let new_protocol_data = builder
        .get_engine_state()
        .get_protocol_data(new_protocol_version)
        .expect("should have result")
        .expect("should have protocol data");

    assert_eq!(new_protocol_data.mint(), old_protocol_data.mint());

    let _default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let wasm_transfer = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_AMOUNT => U512::one(),
            ARG_TARGET => AccountHash::new([1; 32]),
        },
    )
    .with_protocol_version(new_protocol_version)
    .build();

    builder.exec(wasm_transfer).expect_success().commit();
}
