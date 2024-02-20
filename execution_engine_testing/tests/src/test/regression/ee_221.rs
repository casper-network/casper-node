use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::RuntimeArgs;

const CONTRACT_EE_221_REGRESSION: &str = "ee_221_regression.wasm";

#[ignore]
#[test]
fn should_run_ee_221_get_uref_regression_test() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_221_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .expect_success()
        .commit();
}
