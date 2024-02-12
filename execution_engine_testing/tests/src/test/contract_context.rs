use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};

use casper_types::{package::ENTITY_INITIAL_VERSION, runtime_args, Key, RuntimeArgs};

const CONTRACT_HEADERS: &str = "contract_context.wasm";
const PACKAGE_HASH_KEY: &str = "package_hash_key";
const PACKAGE_ACCESS_KEY: &str = "package_access_key";
const CONTRACT_HASH_KEY: &str = "contract_hash_key";

const CONTRACT_CODE_TEST: &str = "contract_code_test";

const NEW_KEY: &str = "new_key";

const CONTRACT_VERSION: &str = "contract_version";

#[ignore]
#[test]
fn should_enforce_intended_execution_contexts() {
    // This test runs a contract that extends the same key with more data after every call.
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HEADERS,
        RuntimeArgs::default(),
    )
    .build();

    let exec_request_3 = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_HASH_KEY,
        Some(ENTITY_INITIAL_VERSION),
        CONTRACT_CODE_TEST,
        runtime_args! {},
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).expect_success().commit();

    builder.exec(exec_request_3).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");

    let _package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    assert!(account.named_keys().get(NEW_KEY).is_none());

    // Check version

    let contract_version_stored = builder
        .query(
            None,
            Key::Account(*DEFAULT_ACCOUNT_ADDR),
            &[CONTRACT_VERSION.to_string()],
        )
        .expect("should query account")
        .as_cl_value()
        .cloned()
        .expect("should be cl value");
    assert_eq!(contract_version_stored.into_t::<u32>().unwrap(), 1u32);
}

#[ignore]
#[test]
fn should_enforce_intended_execution_context_direct_by_name() {
    // This test runs a contract that extends the same key with more data after every call.
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HEADERS,
        RuntimeArgs::default(),
    )
    .build();

    let exec_request_3 = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_KEY,
        CONTRACT_CODE_TEST,
        runtime_args! {},
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).expect_success().commit();

    builder.exec(exec_request_3).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");

    let _package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    assert!(account.named_keys().get(NEW_KEY).is_none());
}

#[ignore]
#[test]
fn should_enforce_intended_execution_context_direct_by_hash() {
    // This test runs a contract that extends the same key with more data after every call.
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HEADERS,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let contract_hash = account
        .named_keys()
        .get(CONTRACT_HASH_KEY)
        .expect("should have contract hash")
        .into_entity_hash();

    let contract_hash = contract_hash.unwrap();

    let exec_request_3 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        CONTRACT_CODE_TEST,
        runtime_args! {},
    )
    .build();

    builder.exec(exec_request_3).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("must have contract");

    let _package_hash = account
        .named_keys()
        .get(PACKAGE_HASH_KEY)
        .expect("should have contract package");
    let _access_uref = account
        .named_keys()
        .get(PACKAGE_ACCESS_KEY)
        .expect("should have package hash");

    assert!(account.named_keys().get(NEW_KEY).is_none())
}
