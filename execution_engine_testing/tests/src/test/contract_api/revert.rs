use casper_engine_test_support::{
    instrumented, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
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
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec_instrumented(exec_request, instrumented!())
        .commit()
        .is_error();
}
