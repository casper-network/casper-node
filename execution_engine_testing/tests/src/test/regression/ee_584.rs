use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_types::{execution::TransformKindV2, RuntimeArgs, StoredValue};

const CONTRACT_EE_584_REGRESSION: &str = "ee_584_regression.wasm";

#[ignore]
#[allow(unused)]
// #[test]
fn should_run_ee_584_no_errored_session_transforms() {
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_EE_584_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(exec_request);

    assert!(builder.is_error());

    let effects = &builder.get_effects()[0];

    assert!(!effects.transforms().iter().any(|transform| {
        if let TransformKindV2::Write(StoredValue::CLValue(cl_value)) = transform.kind() {
            cl_value.to_owned().into_t::<String>().unwrap_or_default() == "Hello, World!"
        } else {
            false
        }
    }));
}
