use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state, execution};
use casper_types::{runtime_args, AccessRights, RuntimeArgs, URef};

const REGRESSION_20220211_CONTRACT: &str = "regression_20220211.wasm";
const REGRESSION_20220211_CALL_CONTRACT: &str = "regression_20220211_call.wasm";
const RET_AS_CONTRACT: &str = "ret_as_contract";
const RET_AS_SESSION: &str = "ret_as_contract";
const PUT_KEY_AS_SESSION: &str = "put_key_as_session";
const PUT_KEY_AS_CONTRACT: &str = "put_key_as_contract";
const READ_AS_SESSION: &str = "read_as_session";
const READ_AS_CONTRACT: &str = "read_as_contract";
const WRITE_AS_SESSION: &str = "write_as_session";
const WRITE_AS_CONTRACT: &str = "write_as_contract";
const ADD_AS_SESSION: &str = "add_as_session";
const ADD_AS_CONTRACT: &str = "add_as_contract";
const ARG_ENTRYPOINT: &str = "entrypoint";

#[ignore]
#[test]
fn regression_20220211_ret_as_contract() {
    test(RET_AS_CONTRACT);
}

#[ignore]
#[test]
fn regression_20220211_ret_as_session() {
    test(RET_AS_SESSION);
}

fn setup() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());
    let install_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        REGRESSION_20220211_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(install_request).expect_success().commit();

    builder
}

fn test(entrypoint: &str) {
    let mut builder = setup();

    let expected_forged_uref = URef::default().with_access_rights(AccessRights::READ_ADD_WRITE);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        REGRESSION_20220211_CALL_CONTRACT,
        runtime_args! {
            ARG_ENTRYPOINT => entrypoint,
        },
    )
    .build();
    builder.exec(exec_request).commit();

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::ForgedReference(forged_uref))
            if forged_uref == expected_forged_uref
        ),
        "Expected revert but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn regression_20220211_put_key_as_session() {
    test(PUT_KEY_AS_SESSION);
}

#[ignore]
#[test]
fn regression_20220211_put_key_as_contract() {
    test(PUT_KEY_AS_CONTRACT);
}

#[ignore]
#[test]
fn regression_20220211_read_as_session() {
    test(READ_AS_SESSION);
}

#[ignore]
#[test]
fn regression_20220211_read_as_contract() {
    test(READ_AS_CONTRACT);
}

#[ignore]
#[test]
fn regression_20220211_write_as_session() {
    test(WRITE_AS_SESSION);
}

#[ignore]
#[test]
fn regression_20220211_write_as_contract() {
    test(WRITE_AS_CONTRACT);
}

#[ignore]
#[test]
fn regression_20220211_add_as_session() {
    test(ADD_AS_SESSION);
}

#[ignore]
#[test]
fn regression_20220211_add_as_contract() {
    test(ADD_AS_CONTRACT);
}
