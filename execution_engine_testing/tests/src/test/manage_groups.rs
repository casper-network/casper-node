use std::collections::BTreeSet;

use assert_matches::assert_matches;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution};
use casper_types::{
    addressable_entity::{self, MAX_GROUPS},
    package::ENTITY_INITIAL_VERSION,
    runtime_args, Group, RuntimeArgs,
};

const CONTRACT_GROUPS: &str = "manage_groups.wasm";
const PACKAGE_HASH_KEY: &str = "package_hash_key";
const PACKAGE_ACCESS_KEY: &str = "package_access_key";
const CREATE_GROUP: &str = "create_group";
const REMOVE_GROUP: &str = "remove_group";
const EXTEND_GROUP_UREFS: &str = "extend_group_urefs";
const REMOVE_GROUP_UREFS: &str = "remove_group_urefs";
const GROUP_NAME_ARG: &str = "group_name";
const NEW_UREFS_COUNT: u64 = 3;
const GROUP_1_NAME: &str = "Group 1";
const TOTAL_NEW_UREFS_ARG: &str = "total_new_urefs";
const TOTAL_EXISTING_UREFS_ARG: &str = "total_existing_urefs";
const ARG_AMOUNT: &str = "amount";
const ARG_UREF_INDICES: &str = "uref_indices";

static DEFAULT_CREATE_GROUP_ARGS: Lazy<RuntimeArgs> = Lazy::new(|| {
    runtime_args! {
        GROUP_NAME_ARG => GROUP_1_NAME,
        TOTAL_NEW_UREFS_ARG => 1u64,
        TOTAL_EXISTING_UREFS_ARG => 1u64,
    }
});

#[ignore]
#[test]
fn should_create_and_remove_group() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(ENTITY_INITIAL_VERSION),
                CREATE_GROUP,
                DEFAULT_CREATE_GROUP_ARGS.clone(),
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_2).expect_success().commit();

    let query_result = builder
        .query(None, *package_hash, &[])
        .expect("should have result");
    let contract_package = query_result.as_package().expect("should be package");
    assert_eq!(contract_package.groups().len(), 1);
    let group_1 = contract_package
        .groups()
        .get(&Group::new(GROUP_1_NAME))
        .expect("should have group");
    assert_eq!(group_1.len(), 2);

    let exec_request_3 = {
        // This inserts package as an argument because this test
        // can work from different accounts which might not have the same keys in their session
        // code.
        let args = runtime_args! {
            GROUP_NAME_ARG => GROUP_1_NAME,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(ENTITY_INITIAL_VERSION),
                REMOVE_GROUP,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).expect_success().commit();

    let query_result = builder
        .query(None, *package_hash, &[])
        .expect("should have result");
    let contract_package = query_result.as_package().expect("should be package");
    assert_eq!(
        contract_package.groups().get(&Group::new(GROUP_1_NAME)),
        None
    );
}

#[ignore]
#[test]
fn should_create_and_extend_user_group() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(ENTITY_INITIAL_VERSION),
                CREATE_GROUP,
                DEFAULT_CREATE_GROUP_ARGS.clone(),
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([5; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_2).expect_success().commit();

    let query_result = builder
        .query(None, *package_hash, &[])
        .expect("should have result");
    let contract_package = query_result.as_package().expect("should be package");
    assert_eq!(contract_package.groups().len(), 1);
    let group_1 = contract_package
        .groups()
        .get(&Group::new(GROUP_1_NAME))
        .expect("should have group");
    assert_eq!(group_1.len(), 2);

    let exec_request_3 = {
        // This inserts package as an argument because this test
        // can work from different accounts which might not have the same keys in their session
        // code.
        let args = runtime_args! {
            GROUP_NAME_ARG => GROUP_1_NAME,
            TOTAL_NEW_UREFS_ARG => NEW_UREFS_COUNT,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(ENTITY_INITIAL_VERSION),
                EXTEND_GROUP_UREFS,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).expect_success().commit();

    let query_result = builder
        .query(None, *package_hash, &[])
        .expect("should have result");
    let contract_package = query_result.as_package().expect("should be package");
    let group_1_extended = contract_package
        .groups()
        .get(&Group::new(GROUP_1_NAME))
        .expect("should have group");
    assert!(group_1_extended.len() > group_1.len());
    // Calculates how many new urefs were created
    let new_urefs: BTreeSet<_> = group_1_extended.difference(group_1).collect();
    assert_eq!(new_urefs.len(), NEW_UREFS_COUNT as usize);
}

#[ignore]
#[test]
fn should_create_and_remove_urefs_from_group() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(ENTITY_INITIAL_VERSION),
                CREATE_GROUP,
                DEFAULT_CREATE_GROUP_ARGS.clone(),
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_2).expect_success().commit();

    let query_result = builder
        .query(None, *package_hash, &[])
        .expect("should have result");
    let contract_package = query_result.as_package().expect("should be package");
    assert_eq!(contract_package.groups().len(), 1);
    let group_1 = contract_package
        .groups()
        .get(&Group::new(GROUP_1_NAME))
        .expect("should have group");
    assert_eq!(group_1.len(), 2);

    let exec_request_3 = {
        // This inserts package as an argument because this test can work from different accounts
        // which might not have the same keys in their session code.
        let args = runtime_args! {
            GROUP_NAME_ARG => GROUP_1_NAME,
            // We're passing indices of urefs inside a group rather than URef values as group urefs
            // aren't part of the access rights. This test will read a ContractPackage instance, get
            // the group by its name, and remove URefs by their indices.
            ARG_UREF_INDICES => vec![0u64, 1u64],
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(ENTITY_INITIAL_VERSION),
                REMOVE_GROUP_UREFS,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).expect_success().commit();

    let query_result = builder
        .query(None, *package_hash, &[])
        .expect("should have result");
    let contract_package = query_result.as_package().expect("should be package");
    let group_1_modified = contract_package
        .groups()
        .get(&Group::new(GROUP_1_NAME))
        .expect("should have group 1");
    assert!(group_1_modified.len() < group_1.len());
}

#[ignore]
#[test]
fn should_limit_max_urefs_while_extending() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_GROUPS,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(ENTITY_INITIAL_VERSION),
                CREATE_GROUP,
                DEFAULT_CREATE_GROUP_ARGS.clone(),
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([3; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_2).expect_success().commit();

    let query_result = builder
        .query(None, *package_hash, &[])
        .expect("should have result");
    let contract_package = query_result.as_package().expect("should be package");
    assert_eq!(contract_package.groups().len(), 1);
    let group_1 = contract_package
        .groups()
        .get(&Group::new(GROUP_1_NAME))
        .expect("should have group");
    assert_eq!(group_1.len(), 2);

    let exec_request_3 = {
        // This inserts package as an argument because this test
        // can work from different accounts which might not have the same keys in their session
        // code.
        let args = runtime_args! {
            GROUP_NAME_ARG => GROUP_1_NAME,
            TOTAL_NEW_UREFS_ARG => 8u64,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(ENTITY_INITIAL_VERSION),
                EXTEND_GROUP_UREFS,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([5; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let exec_request_4 = {
        // This inserts package as an argument because this test
        // can work from different accounts which might not have the same keys in their session
        // code.
        let args = runtime_args! {
            GROUP_NAME_ARG => GROUP_1_NAME,
            // Exceeds by 1
            TOTAL_NEW_UREFS_ARG => 1u64,
        };
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_stored_versioned_contract_by_name(
                PACKAGE_HASH_KEY,
                Some(ENTITY_INITIAL_VERSION),
                EXTEND_GROUP_UREFS,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([32; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder.exec(exec_request_3).expect_success().commit();

    let query_result = builder
        .query(None, *package_hash, &[])
        .expect("should have result");
    let contract_package = query_result.as_package().expect("should be package");
    let group_1_modified = contract_package
        .groups()
        .get(&Group::new(GROUP_1_NAME))
        .expect("should have group 1");
    assert_eq!(group_1_modified.len(), MAX_GROUPS as usize);

    // Tries to exceed the limit by 1
    builder.exec(exec_request_4).commit();

    let response = builder
        .get_last_exec_results()
        .expect("should have last response");
    assert_eq!(response.len(), 1);
    let exec_response = response.last().expect("should have response");
    let error = exec_response.as_error().expect("should have error");
    let error = assert_matches!(error, Error::Exec(execution::Error::Revert(e)) => e);
    assert_eq!(
        error,
        &addressable_entity::Error::MaxTotalURefsExceeded.into()
    );
}
