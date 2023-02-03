use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::RuntimeArgs;

const REGRESSION_20220119_CONTRACT: &str = "regression_20220119.wasm";

#[ignore]
#[test]
fn should_create_purse() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        REGRESSION_20220119_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();
}
