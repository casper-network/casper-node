use assert_matches::assert_matches;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, DEFAULT_RUN_GENESIS_REQUEST, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::core::{engine_state::Error, execution};
use casper_types::{
    account::AccountHash, contracts::CONTRACT_INITIAL_VERSION, runtime_args, Key, RuntimeArgs, U512,
};

use crate::wasm_utils;

const CONTRACT_GROUPS: &str = "groups.wasm";
const PACKAGE_HASH_KEY: &str = "package_hash_key";
const PACKAGE_ACCESS_KEY: &str = "package_access_key";
const RESTRICTED_SESSION: &str = "restricted_session";
const RESTRICTED_CONTRACT: &str = "restricted_contract";
const RESTRICTED_SESSION_CALLER: &str = "restricted_session_caller";
const UNRESTRICTED_CONTRACT_CALLER: &str = "unrestricted_contract_caller";
const PACKAGE_HASH_ARG: &str = "package_hash";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const RESTRICTED_CONTRACT_CALLER_AS_SESSION: &str = "restricted_contract_caller_as_session";
const UNCALLABLE_SESSION: &str = "uncallable_session";
const UNCALLABLE_CONTRACT: &str = "uncallable_contract";
const CALL_RESTRICTED_ENTRY_POINTS: &str = "call_restricted_entry_points";
const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";

static TRANSFER_1_AMOUNT: Lazy<U512> =
    Lazy::new(|| U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE) + 1000);

#[ignore]
#[test]
fn should_call_group_restricted_session() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let _package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_2 = {
        // This inserts package as an argument because this test
        // can work from different accounts which might not have the same keys in their session
        // code.
        let args = runtime_args! {};
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(CONTRACT_INITIAL_VERSION),
                RESTRICTED_SESSION,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_2).expect_success().commit();

    let _account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");
}

#[ignore]
#[test]
fn should_call_group_restricted_session_caller() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_2 = {
        let args = runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(CONTRACT_INITIAL_VERSION),
                RESTRICTED_SESSION_CALLER,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };
    builder.exec(exec_request_2).expect_success().commit();

    let _account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");
}

#[test]
#[ignore]
fn should_not_call_restricted_session_from_wrong_account() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_3 = {
        let args = runtime_args! {};
        let deploy = DeployItemBuilder::new()
            .with_address(ACCOUNT_1_ADDR)
            .with_stored_versioned_contract_by_hash(
                package_hash.into_hash().expect("should be hash"),
                Some(CONTRACT_INITIAL_VERSION),
                RESTRICTED_SESSION,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[ACCOUNT_1_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).commit();

    let _account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let response = builder
        .get_last_exec_results()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::InvalidContext));
}

#[test]
#[ignore]
fn should_not_call_restricted_session_caller_from_wrong_account() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_3 = {
        let args = runtime_args! {
            "package_hash" => *package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(ACCOUNT_1_ADDR)
            .with_stored_versioned_contract_by_hash(
                package_hash.into_hash().expect("should be hash"),
                Some(CONTRACT_INITIAL_VERSION),
                RESTRICTED_SESSION_CALLER,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[ACCOUNT_1_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).commit();

    let _account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let response = builder
        .get_last_exec_results()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::InvalidContext));
}

#[ignore]
#[test]
fn should_call_group_restricted_contract() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_2 = {
        // This inserts package as an argument because this test
        // can work from different accounts which might not have the same keys in their session
        // code.
        let args = runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(CONTRACT_INITIAL_VERSION),
                RESTRICTED_CONTRACT,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_2).expect_success().commit();

    let _account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");
}

#[ignore]
#[test]
fn should_not_call_group_restricted_contract_from_wrong_account() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();
    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_3 = {
        // This inserts package as an argument because this test
        // can work from different accounts which might not have the same keys in their session
        // code.
        let args = runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(ACCOUNT_1_ADDR)
            .with_stored_versioned_contract_by_hash(
                package_hash.into_hash().expect("should be hash"),
                Some(CONTRACT_INITIAL_VERSION),
                RESTRICTED_CONTRACT,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[ACCOUNT_1_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).commit();

    let response = builder
        .get_last_exec_results()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::InvalidContext));
}

#[ignore]
#[test]
fn should_call_group_unrestricted_contract_caller() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_2 = {
        let args = runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(CONTRACT_INITIAL_VERSION),
                UNRESTRICTED_CONTRACT_CALLER,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };
    builder.exec(exec_request_2).expect_success().commit();

    let _account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");
}

#[ignore]
#[test]
fn should_call_unrestricted_contract_caller_from_different_account() {
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();
    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_3 = {
        // This inserts package as an argument because this test
        // can work from different accounts which might not have the same keys in their session
        // code.
        let args = runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(ACCOUNT_1_ADDR)
            .with_stored_versioned_contract_by_hash(
                package_hash.into_hash().expect("should be hash"),
                Some(CONTRACT_INITIAL_VERSION),
                UNRESTRICTED_CONTRACT_CALLER,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[ACCOUNT_1_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).expect_success().commit();
}

#[ignore]
#[test]
fn should_call_group_restricted_contract_as_session() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();
    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_3 = {
        // This inserts package as an argument because this test
        // can work from different accounts which might not have the same keys in their session
        // code.
        let args = runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_hash(
                package_hash.into_hash().expect("should be hash"),
                Some(CONTRACT_INITIAL_VERSION),
                RESTRICTED_CONTRACT_CALLER_AS_SESSION,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([4; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).expect_success().commit();
}

#[ignore]
#[test]
fn should_call_group_restricted_contract_as_session_from_wrong_account() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();
    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_3 = {
        // This inserts package as an argument because this test
        // can work from different accounts which might not have the same keys in their session
        // code.
        let args = runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(ACCOUNT_1_ADDR)
            .with_stored_versioned_contract_by_hash(
                package_hash.into_hash().expect("should be hash"),
                Some(CONTRACT_INITIAL_VERSION),
                RESTRICTED_CONTRACT_CALLER_AS_SESSION,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[ACCOUNT_1_ADDR])
            .with_deploy_hash([4; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).commit();

    let response = builder
        .get_last_exec_results()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::InvalidContext));
}

#[ignore]
#[test]
fn should_not_call_uncallable_contract_from_deploy() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_2 = {
        // This inserts package as an argument because this test
        // can work from different accounts which might not have the same keys in their session
        // code.
        let args = runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(CONTRACT_INITIAL_VERSION),
                UNCALLABLE_SESSION,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_2).commit();
    let response = builder
        .get_last_exec_results()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::InvalidContext));

    let exec_request_3 = {
        let args = runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(CONTRACT_INITIAL_VERSION),
                CALL_RESTRICTED_ENTRY_POINTS,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([6; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).expect_success().commit();
}

#[ignore]
#[test]
fn should_not_call_uncallable_session_from_deploy() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_2 = {
        // This inserts package as an argument because this test
        // can work from different accounts which might not have the same keys in their session
        // code.
        let args = runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(CONTRACT_INITIAL_VERSION),
                UNCALLABLE_CONTRACT,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_2).commit();
    let response = builder
        .get_last_exec_results()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::InvalidContext));

    let exec_request_3 = {
        let args = runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(CONTRACT_INITIAL_VERSION),
                CALL_RESTRICTED_ENTRY_POINTS,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([6; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };
    builder.exec(exec_request_3).expect_success().commit();
}

#[test]
#[ignore]
fn should_not_call_group_restricted_stored_payment_code_from_invalid_account() {
    // This test calls a stored payment code that is restricted with a group access using an account
    // that does not have any of the group urefs in context.

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_3 = {
        let args = runtime_args! {
            "amount" => *DEFAULT_PAYMENT,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(ACCOUNT_1_ADDR)
            .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
            .with_stored_versioned_payment_contract_by_hash(
                package_hash
                    .into_hash()
                    .expect("must have created package hash"),
                Some(CONTRACT_INITIAL_VERSION),
                "restricted_standard_payment",
                args,
            )
            .with_authorization_keys(&[ACCOUNT_1_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).commit();

    let _account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let response = builder
        .get_last_exec_results()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::InvalidContext));
}

#[test]
#[ignore]
fn should_call_group_restricted_stored_payment_code() {
    // This test calls a stored payment code that is restricted with a group access using an account
    // that contains urefs from the group.
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .cloned()
        .expect("should be account");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    let exec_request_3 = {
        let args = runtime_args! {
            "amount" => *DEFAULT_PAYMENT,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
            // .with_stored_versioned_contract_by_name(name, version, entry_point, args)
            .with_stored_versioned_payment_contract_by_hash(
                package_hash
                    .into_hash()
                    .expect("must have created package hash"),
                Some(CONTRACT_INITIAL_VERSION),
                "restricted_standard_payment",
                args,
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).expect_success().commit();
}
