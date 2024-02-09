use assert_matches::assert_matches;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::{engine_state::Error, execution};
use casper_types::{
    account::AccountHash, package::ENTITY_INITIAL_VERSION, runtime_args, Key, RuntimeArgs, U512,
};

use crate::{lmdb_fixture, wasm_utils};

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

const GROUPS_FIXTURE: &str = "groups";

static TRANSFER_1_AMOUNT: Lazy<U512> =
    Lazy::new(|| U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE) + 1000);

fn setup_from_lmdb_fixture() -> LmdbWasmTestBuilder {
    let (builder, _, _) = lmdb_fixture::builder_from_global_state_fixture(GROUPS_FIXTURE);

    builder
}

#[ignore]
#[test]
fn should_call_group_restricted_session() {
    let mut builder = setup_from_lmdb_fixture();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default contract");

    let _package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    // This inserts package as an argument because this test
    // can work from different accounts which might not have the same keys in their session
    // code.
    let exec_request_2 = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_HASH_KEY,
        Some(ENTITY_INITIAL_VERSION),
        RESTRICTED_SESSION,
        runtime_args! {},
    )
    .build();

    builder.exec(exec_request_2).expect_failure();

    builder.assert_error(Error::Exec(execution::Error::InvalidContext))
}

#[ignore]
#[test]
fn should_call_group_restricted_session_caller() {
    let mut builder = setup_from_lmdb_fixture();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default contract package");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    // This inserts package as an argument because this test
    // can work from different accounts which might not have the same keys in their session
    // code.
    let exec_request_2 = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_HASH_KEY,
        Some(ENTITY_INITIAL_VERSION),
        RESTRICTED_SESSION,
        runtime_args! {
            PACKAGE_HASH_ARG => package_hash.into_package_hash()
        },
    )
    .build();

    builder.exec(exec_request_2).expect_failure();

    builder.assert_error(Error::Exec(execution::Error::InvalidContext));
}

#[test]
#[ignore]
fn should_not_call_restricted_session_from_wrong_account() {
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = setup_from_lmdb_fixture();

    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default contract package");
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
                package_hash.into_package_addr().expect("should be hash"),
                Some(ENTITY_INITIAL_VERSION),
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
        .as_cl_value()
        .cloned()
        .expect("should be account");

    let response = builder
        .get_last_exec_result()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::InvalidContext));
}

#[test]
#[ignore]
fn should_not_call_restricted_session_caller_from_wrong_account() {
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = setup_from_lmdb_fixture();

    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");

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
                package_hash.into_package_addr().expect("should be hash"),
                Some(ENTITY_INITIAL_VERSION),
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
        .as_cl_value()
        .cloned()
        .expect("should be account");

    let response = builder
        .get_last_exec_result()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::InvalidContext));
}

#[ignore]
#[test]
fn should_call_group_restricted_contract() {
    let mut builder = setup_from_lmdb_fixture();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default contract package");

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
                Some(ENTITY_INITIAL_VERSION),
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
        .as_cl_value()
        .cloned()
        .expect("should be account");
}

#[ignore]
#[test]
fn should_not_call_group_restricted_contract_from_wrong_account() {
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = setup_from_lmdb_fixture();
    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default contract package");

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
                package_hash.into_package_addr().expect("should be hash"),
                Some(ENTITY_INITIAL_VERSION),
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
        .get_last_exec_result()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::InvalidContext));
}

#[ignore]
#[test]
fn should_call_group_unrestricted_contract_caller() {
    let mut builder = setup_from_lmdb_fixture();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default contract package");

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
                Some(ENTITY_INITIAL_VERSION),
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
        .as_cl_value()
        .cloned()
        .expect("should be account");
}

#[ignore]
#[test]
fn should_call_unrestricted_contract_caller_from_different_account() {
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = setup_from_lmdb_fixture();
    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    // This inserts package as an argument because this test
    // can work from different accounts which might not have the same keys in their session
    // code.
    let exec_request_3 = ExecuteRequestBuilder::versioned_contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        package_hash
            .into_package_hash()
            .expect("must have package hash"),
        Some(ENTITY_INITIAL_VERSION),
        UNRESTRICTED_CONTRACT_CALLER,
        runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        },
    )
    .build();

    builder.exec(exec_request_3).expect_success().commit();
}

#[ignore]
#[test]
fn should_call_group_restricted_contract_as_session() {
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = setup_from_lmdb_fixture();
    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default contract package");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    // This inserts package as an argument because this test
    // can work from different accounts which might not have the same keys in their session
    // code.
    let exec_request_3 = ExecuteRequestBuilder::versioned_contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        package_hash
            .into_package_hash()
            .expect("must convert to package hash"),
        Some(ENTITY_INITIAL_VERSION),
        RESTRICTED_CONTRACT_CALLER_AS_SESSION,
        runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        },
    )
    .build();

    builder.exec(exec_request_3).expect_failure();

    builder.assert_error(Error::Exec(execution::Error::InvalidContext))
}

#[ignore]
#[test]
fn should_call_group_restricted_contract_as_session_from_wrong_account() {
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = setup_from_lmdb_fixture();
    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");

    let package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    // This inserts package as an argument because this test
    // can work from different accounts which might not have the same keys in their session
    // code.
    let exec_request_3 = ExecuteRequestBuilder::versioned_contract_call_by_hash(
        ACCOUNT_1_ADDR,
        package_hash
            .into_package_hash()
            .expect("must convert to package hash"),
        Some(ENTITY_INITIAL_VERSION),
        RESTRICTED_CONTRACT_CALLER_AS_SESSION,
        runtime_args! {
            PACKAGE_HASH_ARG => *package_hash,
        },
    )
    .build();

    builder.exec(exec_request_3).commit();

    let response = builder
        .get_last_exec_result()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    assert_matches!(error, Error::Exec(execution::Error::InvalidContext));
}

#[ignore]
#[test]
fn should_not_call_uncallable_contract_from_deploy() {
    let mut builder = setup_from_lmdb_fixture();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default contract package");

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
                Some(ENTITY_INITIAL_VERSION),
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
        .get_last_exec_result()
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
                Some(ENTITY_INITIAL_VERSION),
                CALL_RESTRICTED_ENTRY_POINTS,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([6; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).expect_failure();

    builder.assert_error(Error::Exec(execution::Error::InvalidContext))
}

#[ignore]
#[test]
fn should_not_call_uncallable_session_from_deploy() {
    let mut builder = setup_from_lmdb_fixture();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");

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
                Some(ENTITY_INITIAL_VERSION),
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
        .get_last_exec_result()
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
                Some(ENTITY_INITIAL_VERSION),
                CALL_RESTRICTED_ENTRY_POINTS,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([6; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };
    builder.exec(exec_request_3).expect_failure();

    builder.assert_error(Error::Exec(execution::Error::InvalidContext))
}

#[test]
#[ignore]
fn should_not_call_group_restricted_stored_payment_code_from_invalid_account() {
    // This test calls a stored payment code that is restricted with a group access using an account
    // that does not have any of the group urefs in context.

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();
    let mut builder = setup_from_lmdb_fixture();

    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default contract package");

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
                    .into_package_addr()
                    .expect("must have created package hash"),
                Some(ENTITY_INITIAL_VERSION),
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
        .as_cl_value()
        .cloned()
        .expect("should be account");

    let response = builder
        .get_last_exec_result()
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

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let mut builder = setup_from_lmdb_fixture();

    builder.exec(exec_request_2).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have default contract package");

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
                    .into_package_addr()
                    .expect("must have created package hash"),
                Some(ENTITY_INITIAL_VERSION),
                "restricted_standard_payment",
                args,
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).expect_failure();

    builder.assert_error(Error::Exec(execution::Error::InvalidContext));
}
