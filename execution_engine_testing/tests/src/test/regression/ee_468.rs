use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution::ExecError};
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

    let error = LmdbWasmTestBuilder::default()
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .commit()
        .get_error();

    assert!(
        matches!(
            error,
            Some(Error::Exec(ExecError::BytesRepr(
                bytesrepr::Error::EarlyEndOfStream
            )))
        ),
        "{:?}",
        error
    );
}
