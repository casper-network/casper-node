use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::shared::transform::Transform;
use casper_types::{RuntimeArgs, StoredValue};

const CONTRACT_EE_584_REGRESSION: &str = "ee_584_regression.wasm";

#[ignore]
#[test]
fn should_run_ee_584_no_errored_session_transforms() {
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_584_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request);

    assert!(builder.is_error());

    let transforms = builder.get_execution_journals();

    assert!(!transforms[0].iter().any(|(_, t)| {
        if let Transform::Write(StoredValue::CLValue(cl_value)) = t {
            cl_value.to_owned().into_t::<String>().unwrap_or_default() == "Hello, World!"
        } else {
            false
        }
    }));
}
