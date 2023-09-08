use casper_engine_test_support::{
    instrumented, ExecuteRequestBuilder, InMemoryWasmTestBuilder, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::engine_state::Error;
use casper_types::{account::AccountHash, RuntimeArgs};

const CONTRACT_EE_532_REGRESSION: &str = "ee_532_regression.wasm";
const UNKNOWN_ADDR: AccountHash = AccountHash::new([42u8; 32]);

#[ignore]
#[test]
fn should_run_ee_532_get_uref_regression_test() {
    // This test runs a contract that's after every call extends the same key with
    // more data

    let exec_request = ExecuteRequestBuilder::standard(
        UNKNOWN_ADDR,
        CONTRACT_EE_532_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();
    builder
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec_instrumented(exec_request, instrumented!())
        .commit();

    let deploy_result = builder
        .get_exec_result_owned(0)
        .expect("should have exec response")
        .get(0)
        .cloned()
        .expect("should have at least one deploy result");

    assert!(
        deploy_result.has_precondition_failure(),
        "expected precondition failure"
    );

    let message = deploy_result.as_error().map(|err| format!("{}", err));
    assert_eq!(
        message,
        Some(format!("{}", Error::Authorization)),
        "expected Error::Authorization"
    )
}
