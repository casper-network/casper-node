use casper_engine_test_support::{
    instrumented, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{account::Weight, runtime_args, RuntimeArgs};

const CONTRACT_EE_539_REGRESSION: &str = "ee_539_regression.wasm";
const ARG_KEY_MANAGEMENT_THRESHOLD: &str = "key_management_threshold";
const ARG_DEPLOYMENT_THRESHOLD: &str = "deployment_threshold";

#[ignore]
#[test]
fn should_run_ee_539_serialize_action_thresholds_regression() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request = ExecuteRequestBuilder::standard(
       *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_539_REGRESSION,
        runtime_args! { ARG_KEY_MANAGEMENT_THRESHOLD => Weight::new(4), ARG_DEPLOYMENT_THRESHOLD => Weight::new(3) },
    )
        .build();

    let mut builder = InMemoryWasmTestBuilder::default();
    builder
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec_instrumented(exec_request, instrumented!())
        .expect_success()
        .commit();
}
