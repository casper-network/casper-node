use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::{engine_state, execution};
use casper_types::{bytesrepr, RuntimeArgs};

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
    let error = InMemoryWasmTestBuilder::default()
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .commit()
        .get_error();

    assert!(
        matches!(
            error,
            Some(engine_state::Error::Exec(execution::Error::BytesRepr(
                bytesrepr::Error::EarlyEndOfStream
            )))
        ),
        "{:?}",
        error
    );
}
