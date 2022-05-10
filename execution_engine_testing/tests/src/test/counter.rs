use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{runtime_args, Key, RuntimeArgs};

const CONTRACT_COUNTER_DEFINE: &str = "counter_define.wasm";
const CONTRACT_NAME: &str = "counter_package_hash";
const COUNTER_VALUE_UREF: &str = "counter";
const ENTRYPOINT_COUNTER: &str = "counter";
const ENTRYPOINT_SESSION: &str = "session";
const COUNTER_CONTRACT_HASH_KEY_NAME: &str = "counter_contract_hash";
const ARG_COUNTER_METHOD: &str = "method";
const METHOD_INC: &str = "inc";

#[ignore]
#[test]
fn should_run_counter_example_contract() {
    let mut builder = InMemoryWasmTestBuilder::default();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_COUNTER_DEFINE,
        RuntimeArgs::new(),
    )
    .build();

    builder
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .expect_success()
        .commit();

    let account = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("should query account")
        .as_account()
        .expect("should be account")
        .clone();

    let counter_contract_hash_key = *account
        .named_keys()
        .get(COUNTER_CONTRACT_HASH_KEY_NAME)
        .expect("should have counter contract hash key");

    let exec_request_2 = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NAME,
        None,
        ENTRYPOINT_SESSION,
        runtime_args! { COUNTER_CONTRACT_HASH_KEY_NAME => counter_contract_hash_key },
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();

    let value: i32 = builder
        .query(
            None,
            counter_contract_hash_key,
            &[COUNTER_VALUE_UREF.to_string()],
        )
        .expect("should have counter value")
        .as_cl_value()
        .expect("should be CLValue")
        .clone()
        .into_t()
        .expect("should cast CLValue to integer");

    assert_eq!(value, 1);

    let exec_request_3 = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NAME,
        None,
        ENTRYPOINT_SESSION,
        runtime_args! { COUNTER_CONTRACT_HASH_KEY_NAME => counter_contract_hash_key },
    )
    .build();

    builder.exec(exec_request_3).expect_success().commit();

    let value: i32 = builder
        .query(
            None,
            counter_contract_hash_key,
            &[COUNTER_VALUE_UREF.to_string()],
        )
        .expect("should have counter value")
        .as_cl_value()
        .expect("should be CLValue")
        .clone()
        .into_t()
        .expect("should cast CLValue to integer");

    assert_eq!(value, 2);
}

#[ignore]
#[test]
fn should_default_contract_hash_arg() {
    let mut builder = InMemoryWasmTestBuilder::default();

    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_COUNTER_DEFINE,
        RuntimeArgs::new(),
    )
    .build();

    builder
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .expect_success()
        .commit();

    let exec_request_2 = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NAME,
        None,
        ENTRYPOINT_SESSION,
        RuntimeArgs::new(),
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();

    let value: i32 = {
        let counter_contract_hash_key = *builder
            .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
            .expect("should query account")
            .as_account()
            .expect("should be account")
            .clone()
            .named_keys()
            .get(COUNTER_CONTRACT_HASH_KEY_NAME)
            .expect("should have counter contract hash key");

        builder
            .query(
                None,
                counter_contract_hash_key,
                &[COUNTER_VALUE_UREF.to_string()],
            )
            .expect("should have counter value")
            .as_cl_value()
            .expect("should be CLValue")
            .clone()
            .into_t()
            .expect("should cast CLValue to integer")
    };

    assert_eq!(value, 1);
}

#[ignore]
#[test]
fn should_call_counter_contract_directly() {
    let mut builder = InMemoryWasmTestBuilder::default();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_COUNTER_DEFINE,
        RuntimeArgs::new(),
    )
    .build();

    builder
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .expect_success()
        .commit();

    let exec_request_2 = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NAME,
        None,
        ENTRYPOINT_COUNTER,
        runtime_args! { ARG_COUNTER_METHOD => METHOD_INC },
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();

    let value: i32 = {
        let counter_contract_hash_key = *builder
            .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
            .expect("should query account")
            .as_account()
            .expect("should be account")
            .clone()
            .named_keys()
            .get(COUNTER_CONTRACT_HASH_KEY_NAME)
            .expect("should have counter contract hash key");

        builder
            .query(
                None,
                counter_contract_hash_key,
                &[COUNTER_VALUE_UREF.to_string()],
            )
            .expect("should have counter value")
            .as_cl_value()
            .expect("should be CLValue")
            .clone()
            .into_t()
            .expect("should cast CLValue to integer")
    };

    assert_eq!(value, 1);
}
