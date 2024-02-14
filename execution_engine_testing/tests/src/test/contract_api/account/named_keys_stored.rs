use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{runtime_args, ApiError, RuntimeArgs};

const CONTRACT_HASH_NAME: &str = "contract_stored";
const ENTRY_POINT_CONTRACT: &str = "named_keys_contract";
const ENTRY_POINT_SESSION: &str = "named_keys_session";
const ENTRY_POINT_CONTRACT_TO_CONTRACT: &str = "named_keys_contract_to_contract";

#[ignore]
#[test]
fn should_run_stored_named_keys_contract() {
    let mut builder = setup();
    let exec_request_1 = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_NAME,
        ENTRY_POINT_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();
}

#[ignore]
#[test]
fn should_run_stored_named_keys_session() {
    let mut builder = setup();

    let exec_request_1 = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_NAME,
        ENTRY_POINT_SESSION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request_1).expect_failure();

    let expected_error = casper_execution_engine::engine_state::Error::Exec(
        casper_execution_engine::execution::Error::Revert(ApiError::User(0)),
    );

    builder.assert_error(expected_error)
}

#[ignore]
#[test]
fn should_run_stored_named_keys_contract_to_contract() {
    let mut builder = setup();
    let exec_request_1 = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_HASH_NAME,
        ENTRY_POINT_CONTRACT_TO_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();
}

#[ignore]
#[test]
fn should_run_stored_named_keys_module_bytes_to_contract() {
    let mut builder = setup();
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        "named_keys_stored_call.wasm",
        runtime_args! {
            "entry_point" => ENTRY_POINT_CONTRACT,
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();
}

#[ignore]
#[test]
fn should_run_stored_named_keys_module_bytes_to_contract_to_contract() {
    let mut builder = setup();
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        "named_keys_stored_call.wasm",
        runtime_args! {
            "entry_point" => ENTRY_POINT_CONTRACT_TO_CONTRACT,
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();
}

fn setup() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        "named_keys_stored.wasm",
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(exec_request_1).expect_success().commit();
    builder
}
