use casper_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    DEFAULT_ACCOUNT_ADDR,
};
use casper_types::RuntimeArgs;

const CONTRACT_EE_771_REGRESSION: &str = "ee_771_regression.wasm";

#[ignore]
#[test]
fn should_run_ee_771_regression() {
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_771_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let result = InMemoryWasmTestBuilder::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .commit()
        .finish();

    let response = result
        .builder()
        .get_exec_result(0)
        .expect("should have a response")
        .to_owned();

    let error = response[0].as_error().expect("should have error");
    assert_eq!(
        format!("{}", error),
        "Function not found: functiondoesnotexist"
    );
}
