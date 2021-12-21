use casper_engine_test_support::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, PRODUCTION_PATH};
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

    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);

    builder
        .run_genesis_with_default_genesis_accounts()
        .exec(exec_request)
        .commit();

    let deploy_result = builder
        .get_exec_result(0)
        .expect("should have exec response")
        .get(0)
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
