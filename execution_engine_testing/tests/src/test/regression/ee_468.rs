use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::RuntimeArgs;

const CONTRACT_DESERIALIZE_ERROR: &str = "deserialize_error.wasm";

#[ignore]
#[test]
fn should_not_fail_deserializing() {
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_DESERIALIZE_ERROR,
        RuntimeArgs::new(),
    )
    .build();
    let is_error = InMemoryWasmTestBuilder::default()
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .commit()
        .is_error();

    assert!(is_error);
}
