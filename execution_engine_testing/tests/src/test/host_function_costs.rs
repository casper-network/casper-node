use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{bytesrepr::Bytes, runtime_args, AddressableEntityHash, RuntimeArgs};

const HOST_FUNCTION_COSTS_NAME: &str = "host_function_costs.wasm";
const CONTRACT_KEY_NAME: &str = "contract";

const DO_NOTHING_NAME: &str = "do_nothing";
const DO_SOMETHING_NAME: &str = "do_something";
const CALLS_DO_NOTHING_LEVEL1_NAME: &str = "calls_do_nothing_level1";
const CALLS_DO_NOTHING_LEVEL2_NAME: &str = "calls_do_nothing_level2";
const ARG_BYTES: &str = "bytes";
const ARG_SIZE_FUNCTION_CALL_1_NAME: &str = "arg_size_function_call_1";
const ARG_SIZE_FUNCTION_CALL_100_NAME: &str = "arg_size_function_call_100";

#[ignore]
#[test]
fn should_measure_gas_cost() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let mut builder = LmdbWasmTestBuilder::default();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        HOST_FUNCTION_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    // Create Accounts
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash_addr()
        .expect("should be hash")
        .into();

    //
    // Measure do nothing
    //

    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        DO_NOTHING_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();

    let do_nothing_cost = builder.last_exec_gas_cost().value();

    //
    // Measure opcodes (doing something)
    //
    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        DO_SOMETHING_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();

    let do_something_cost = builder.last_exec_gas_cost().value();
    assert!(
        !do_something_cost.is_zero(),
        "executing nothing should cost zero"
    );
    assert!(do_something_cost > do_nothing_cost);
}

#[ignore]
#[test]
fn should_measure_nested_host_function_call_cost() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let mut builder = LmdbWasmTestBuilder::default();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        HOST_FUNCTION_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    // Create Accounts
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash_addr()
        .expect("should be hash")
        .into();

    //
    // Measure level 1 - nested call to 'do nothing'
    //

    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        CALLS_DO_NOTHING_LEVEL1_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();
    let level_1_cost = builder.last_exec_gas_cost().value();

    assert!(
        !level_1_cost.is_zero(),
        "executing nested call should not cost zero"
    );

    //
    // Measure level 2 - call to an entrypoint that calls 'do nothing'
    //

    let exec_request_3 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        CALLS_DO_NOTHING_LEVEL2_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request_3).expect_success().commit();
    let level_2_cost = builder.last_exec_gas_cost().value();

    assert!(
        !level_2_cost.is_zero(),
        "executing nested call should not cost zero"
    );

    assert!(
        level_2_cost > level_1_cost,
        "call to level2 should be greater than level1 call but {} <= {}",
        level_2_cost,
        level_1_cost,
    );
}

#[ignore]
#[test]
fn should_measure_argument_size_in_host_function_call() {
    // Checks if calling a contract with large arguments affects costs
    let mut builder = LmdbWasmTestBuilder::default();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        HOST_FUNCTION_COSTS_NAME,
        RuntimeArgs::default(),
    )
    .build();

    // Create Accounts
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let contract_hash: AddressableEntityHash = account
        .named_keys()
        .get(CONTRACT_KEY_NAME)
        .expect("contract hash")
        .into_entity_hash_addr()
        .expect("should be hash")
        .into();

    //
    // Measurement 1 - empty vector (argument with 0 bytes value)
    //
    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        ARG_SIZE_FUNCTION_CALL_1_NAME,
        runtime_args! {
            ARG_BYTES => Bytes::new(),
        },
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();
    let call_1_cost = builder.last_exec_gas_cost().value();

    assert!(
        !call_1_cost.is_zero(),
        "executing nested call should not cost zero"
    );

    //
    // Measurement  level 2 - argument that's vector of 100 bytes
    //

    let exec_request_3 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        ARG_SIZE_FUNCTION_CALL_100_NAME,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(exec_request_3).expect_success().commit();
    let call_2_cost = builder.last_exec_gas_cost().value();

    assert!(
        call_2_cost > call_1_cost,
        "call 1 {} call 2 {}",
        call_1_cost,
        call_2_cost
    );
}
