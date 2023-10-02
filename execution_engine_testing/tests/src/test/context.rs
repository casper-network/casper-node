use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::runtime_args;

#[ignore]
#[test]
fn context_key_should_work() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let exec_reqeust = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        "context_key.wasm",
        runtime_args! {},
    )
    .build();

    builder.exec(exec_reqeust).commit().expect_success();
}
