use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, PRODUCTION_PATH,
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

    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);

    builder
        .run_genesis_with_default_genesis_accounts()
        .exec(exec_request)
        .expect_success()
        .commit();
}
