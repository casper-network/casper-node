use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_types::RuntimeArgs;

const REVERT_WASM: &str = "revert.wasm";

#[ignore]
#[test]
fn should_revert() {
    let exec_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, REVERT_WASM, RuntimeArgs::default())
            .build();
    InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .commit()
        .is_error();
}
