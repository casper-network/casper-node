use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{Key, RuntimeArgs, StoredValue};

const COUNT_KEY: &str = "count";
const COUNTER_INSTALLER_WASM: &str = "counter_installer.wasm";
const INCREMENT_COUNTER_WASM: &str = "increment_counter.wasm";
const COUNTER_KEY: &str = "counter";

#[ignore]
#[test]
fn should_run_counter_example() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let install_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        COUNTER_INSTALLER_WASM,
        RuntimeArgs::default(),
    )
    .build();

    let inc_request_1 = ExecuteRequestBuilder::contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        COUNTER_KEY,
        "counter_inc",
        RuntimeArgs::default(),
    )
    .build();

    let call_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        INCREMENT_COUNTER_WASM,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_request_1).expect_success().commit();

    let query_result = builder
        .query(
            None,
            Key::from(*DEFAULT_ACCOUNT_ADDR),
            &[COUNTER_KEY.into(), COUNT_KEY.into()],
        )
        .expect("should query");

    let counter_before: i32 = if let StoredValue::CLValue(cl_value) = query_result {
        cl_value.into_t().unwrap()
    } else {
        panic!("Stored value is not an i32: {:?}", query_result);
    };

    builder.exec(inc_request_1).expect_success().commit();

    let query_result = builder
        .query(
            None,
            Key::from(*DEFAULT_ACCOUNT_ADDR),
            &[COUNTER_KEY.into(), COUNT_KEY.into()],
        )
        .expect("should query");

    let counter_after: i32 = if let StoredValue::CLValue(cl_value) = query_result {
        cl_value.into_t().unwrap()
    } else {
        panic!("Stored value is not an i32: {:?}", query_result);
    };

    let counter_diff = counter_after - counter_before;
    assert_eq!(counter_diff, 1);

    builder.exec(call_request_1).expect_success().commit();
}
