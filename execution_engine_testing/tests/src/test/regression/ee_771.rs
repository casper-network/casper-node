use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, PRODUCTION_PATH,
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

    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);

    builder
        .run_genesis_with_default_genesis_accounts()
        .exec(exec_request)
        .commit();

    let response = builder.get_exec_result(0).expect("should have a response");

    let error = response[0].as_error().expect("should have error");
    assert_eq!(
        format!("{}", error),
        "Function not found: functiondoesnotexist"
    );
}
