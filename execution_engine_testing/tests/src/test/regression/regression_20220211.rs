use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::{engine_state, execution};
use casper_types::{runtime_args, AccessRights, RuntimeArgs, URef};

const REGRESSION_20220211_CONTRACT: &str = "regression_20220211.wasm";
const REGRESSION_20220211_CALL_CONTRACT: &str = "regression_20220211_call.wasm";
const RET_AS_CONTRACT: &str = "ret_as_contract";
const RET_AS_SESSION: &str = "ret_as_session";
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

fn setup() -> InMemoryWasmTestBuilder {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);
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
