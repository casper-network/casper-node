use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, PRODUCTION_PATH,
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

    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);

    builder
        .run_genesis_with_default_genesis_accounts()
        .exec(exec_request)
        .expect_success()
        .commit();
}
