use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, PRODUCTION_PATH,
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

    let mut builder = InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);

    builder
        .run_genesis_with_default_genesis_accounts()
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
