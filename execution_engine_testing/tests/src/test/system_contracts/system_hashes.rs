use casper_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    DEFAULT_ACCOUNT_ADDR,
};
use casper_types::RuntimeArgs;

const CONTRACT_SYSTEM_HASHES: &str = "system_hashes.wasm";

#[ignore]
#[test]
fn should_verify_fixed_system_contract_hashes() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_SYSTEM_HASHES,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request).expect_success().commit();
}
