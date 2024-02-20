use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::Error;
use casper_storage::tracking_copy::TrackingCopyError;
use casper_types::{account::AccountHash, RuntimeArgs};

const CONTRACT_EE_532_REGRESSION: &str = "ee_532_regression.wasm";
const UNKNOWN_ADDR: AccountHash = AccountHash::new([42u8; 32]);

#[ignore]
#[test]
fn should_run_ee_532_non_existent_account_regression_test() {
    let exec_request = ExecuteRequestBuilder::standard(
        UNKNOWN_ADDR,
        CONTRACT_EE_532_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request)
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
        Some(format!(
            "{}",
            Error::TrackingCopy(TrackingCopyError::AccountNotFound(UNKNOWN_ADDR.into()))
        )),
        "expected Error::Authorization"
    )
}
