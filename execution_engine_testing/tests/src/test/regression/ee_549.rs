use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_types::RuntimeArgs;

const CONTRACT_EE_549_REGRESSION: &str = "ee_549_regression.wasm";

#[ignore]
#[test]
fn should_run_ee_549_set_refund_regression() {
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_549_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request);

    // Execution should encounter an error because set_refund
    // is not allowed to be called during session execution.
    assert!(builder.is_error());
}
