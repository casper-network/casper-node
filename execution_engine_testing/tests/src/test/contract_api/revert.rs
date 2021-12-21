use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, PRODUCTION_PATH,
};
use casper_types::RuntimeArgs;

const REVERT_WASM: &str = "revert.wasm";

#[ignore]
#[test]
fn should_revert() {
    let exec_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, REVERT_WASM, RuntimeArgs::default())
            .build();
    InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None)
        .run_genesis_with_default_genesis_accounts()
        .exec(exec_request)
        .commit()
        .is_error();
}
