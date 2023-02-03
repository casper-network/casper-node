use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{runtime_args, RuntimeArgs};

const CONTRACT_HASH_NAME: &str = "contract_stored";
const ENTRY_POINT_CONTRACT: &str = "named_keys_contract";
const ENTRY_POINT_SESSION: &str = "named_keys_session";
const ENTRY_POINT_CONTRACT_TO_CONTRACT: &str = "named_keys_contract_to_contract";
const ENTRY_POINT_SESSION_TO_SESSION: &str = "named_keys_session_to_session";

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

    builder.exec(exec_request_1).expect_success().commit();
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
fn should_run_stored_named_keys_module_bytes_to_session() {
    let mut builder = setup();
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        "named_keys_stored_call.wasm",
        runtime_args! {
            "entry_point" => ENTRY_POINT_SESSION,
        },
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

#[ignore]
#[test]
fn should_run_stored_named_keys_module_bytes_to_session_to_session() {
    let mut builder = setup();
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        "named_keys_stored_call.wasm",
        runtime_args! {
            "entry_point" => ENTRY_POINT_SESSION_TO_SESSION,
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();
}

fn setup() -> InMemoryWasmTestBuilder {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        "named_keys_stored.wasm",
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(exec_request_1).expect_success().commit();
    builder
}
