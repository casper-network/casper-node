use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{
    runtime_args, AddressableEntityHash, EntityVersionKey, PackageHash, RuntimeArgs,
};
use gh_1470_regression::PACKAGE_HASH_NAME;

const GH_3097_REGRESSION_WASM: &str = "gh_3097_regression.wasm";
const GH_3097_REGRESSION_CALL_WASM: &str = "gh_3097_regression_call.wasm";
const DO_SOMETHING_ENTRYPOINT: &str = "do_something";
const DISABLED_CONTRACT_HASH_KEY: &str = "disabled_contract_hash";
const ENABLED_CONTRACT_HASH_KEY: &str = "enabled_contract_hash";
const CONTRACT_PACKAGE_HASH_KEY: &str = "contract_package_hash";
const ARG_METHOD: &str = "method";
const ARG_CONTRACT_HASH_KEY: &str = "contract_hash_key";
const ARG_CONTRACT_VERSION: &str = "contract_version";
const METHOD_CALL_CONTRACT: &str = "call_contract";
const METHOD_CALL_VERSIONED_CONTRACT: &str = "call_versioned_contract";

#[ignore]
#[test]
fn should_run_regression() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_3097_REGRESSION_WASM,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .expect_success()
        .commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let disabled_contract_hash = account
        .named_keys()
        .get(DISABLED_CONTRACT_HASH_KEY)
        .unwrap()
        .into_entity_hash_addr()
        .map(AddressableEntityHash::new)
        .unwrap();
    let enabled_contract_hash = account
        .named_keys()
        .get(ENABLED_CONTRACT_HASH_KEY)
        .unwrap()
        .into_entity_hash_addr()
        .map(AddressableEntityHash::new)
        .unwrap();
    let contract_package_hash = account
        .named_keys()
        .get(CONTRACT_PACKAGE_HASH_KEY)
        .unwrap()
        .into_package_addr()
        .map(PackageHash::new)
        .unwrap();

    // Versioned contract calls by name

    let direct_call_latest_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_HASH_NAME,
        None,
        DO_SOMETHING_ENTRYPOINT,
        RuntimeArgs::new(),
    )
    .build();

    let direct_call_v2_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_HASH_NAME,
        Some(2),
        DO_SOMETHING_ENTRYPOINT,
        RuntimeArgs::new(),
    )
    .build();

    let direct_call_v1_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_HASH_NAME,
        Some(1),
        DO_SOMETHING_ENTRYPOINT,
        RuntimeArgs::new(),
    )
    .build();

    builder
        .exec(direct_call_latest_request)
        .expect_success()
        .commit();

    builder
        .exec(direct_call_v2_request)
        .expect_success()
        .commit();

    builder
        .exec(direct_call_v1_request)
        .expect_failure()
        .commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidEntityVersion(version)
            )
            if version == EntityVersionKey::new(1, 1),
        ),
        "Expected invalid contract version, found {:?}",
        error,
    );

    // Versioned contract calls by hash

    let direct_call_latest_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_package_hash,
        None,
        DO_SOMETHING_ENTRYPOINT,
        RuntimeArgs::new(),
    )
    .build();

    let direct_call_v2_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_package_hash,
        Some(2),
        DO_SOMETHING_ENTRYPOINT,
        RuntimeArgs::new(),
    )
    .build();

    let direct_call_v1_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_package_hash,
        Some(1),
        DO_SOMETHING_ENTRYPOINT,
        RuntimeArgs::new(),
    )
    .build();

    builder
        .exec(direct_call_latest_request)
        .expect_success()
        .commit();

    builder
        .exec(direct_call_v2_request)
        .expect_success()
        .commit();

    builder
        .exec(direct_call_v1_request)
        .expect_failure()
        .commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidEntityVersion(version)
            )
            if version == EntityVersionKey::new(1, 1),
        ),
        "Expected invalid contract version, found {:?}",
        error,
    );

    // Versioned call from a session wasm

    let session_call_v1_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_3097_REGRESSION_CALL_WASM,
        runtime_args! {
            ARG_METHOD => METHOD_CALL_VERSIONED_CONTRACT,
            ARG_CONTRACT_VERSION => Some(1u32),
        },
    )
    .build();

    let session_call_v2_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_3097_REGRESSION_CALL_WASM,
        runtime_args! {
            ARG_METHOD => METHOD_CALL_VERSIONED_CONTRACT,
            ARG_CONTRACT_VERSION => Some(2u32),
        },
    )
    .build();

    let session_call_latest_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_3097_REGRESSION_CALL_WASM,
        runtime_args! {
            ARG_METHOD => METHOD_CALL_VERSIONED_CONTRACT,
            ARG_CONTRACT_VERSION => Option::<u32>::None,
        },
    )
    .build();

    builder
        .exec(session_call_latest_request)
        .expect_success()
        .commit();

    builder
        .exec(session_call_v2_request)
        .expect_success()
        .commit();

    builder
        .exec(session_call_v1_request)
        .expect_failure()
        .commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidEntityVersion(version)
            )
            if version == EntityVersionKey::new(1, 1),
        ),
        "Expected invalid contract version, found {:?}",
        error,
    );

    // Call by contract hashes

    let call_by_hash_v2_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        enabled_contract_hash,
        DO_SOMETHING_ENTRYPOINT,
        RuntimeArgs::new(),
    )
    .build();

    builder
        .exec(call_by_hash_v2_request)
        .expect_success()
        .commit();

    let call_by_name_v2_request = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        ENABLED_CONTRACT_HASH_KEY,
        DO_SOMETHING_ENTRYPOINT,
        RuntimeArgs::new(),
    )
    .build();
    builder
        .exec(call_by_name_v2_request)
        .expect_success()
        .commit();

    // This direct contract by name/hash should fail
    let call_by_hash_v1_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        disabled_contract_hash,
        DO_SOMETHING_ENTRYPOINT,
        RuntimeArgs::new(),
    )
    .build();
    builder
        .exec(call_by_hash_v1_request)
        .expect_failure()
        .commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::DisabledEntity(contract_hash)
            )
            if contract_hash == disabled_contract_hash
        ),
        "Expected invalid contract version, found {:?}",
        error,
    );

    // This direct contract by name/hash should fail
    let call_by_name_v1_request = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        DISABLED_CONTRACT_HASH_KEY,
        DO_SOMETHING_ENTRYPOINT,
        RuntimeArgs::new(),
    )
    .build();
    builder
        .exec(call_by_name_v1_request)
        .expect_failure()
        .commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::DisabledEntity(contract_hash)
            )
            if contract_hash == disabled_contract_hash
        ),
        "Expected invalid contract version, found {:?}",
        error,
    );

    // Session calls into hashes

    let session_call_hash_v1_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_3097_REGRESSION_CALL_WASM,
        runtime_args! {
            ARG_METHOD => METHOD_CALL_CONTRACT,
            ARG_CONTRACT_HASH_KEY => DISABLED_CONTRACT_HASH_KEY,
        },
    )
    .build();

    let session_call_hash_v2_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        GH_3097_REGRESSION_CALL_WASM,
        runtime_args! {
            ARG_METHOD => METHOD_CALL_CONTRACT,
            ARG_CONTRACT_HASH_KEY => ENABLED_CONTRACT_HASH_KEY,
        },
    )
    .build();

    builder
        .exec(session_call_hash_v1_request)
        .expect_failure()
        .commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(
            error,
            casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::DisabledEntity(contract_hash)
            )
            if contract_hash == disabled_contract_hash
        ),
        "Expected invalid contract version, found {:?}",
        error,
    );

    builder
        .exec(session_call_hash_v2_request)
        .expect_success()
        .commit();
}
