use num_traits::cast::AsPrimitive;

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{contracts::CONTRACT_INITIAL_VERSION, runtime_args, RuntimeArgs, U512};

const ARG_TARGET: &str = "target_contract";
const ARG_GAS_AMOUNT: &str = "gas_amount";
const ARG_METHOD_NAME: &str = "method_name";

#[ignore]
#[test]
fn should_charge_gas_for_subcall() {
    const CONTRACT_NAME: &str = "measure_gas_subcall.wasm";
    const DO_NOTHING: &str = "do-nothing";
    const DO_SOMETHING: &str = "do-something";
    const NO_SUBCALL: &str = "no-subcall";

    let do_nothing_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NAME,
        runtime_args! { ARG_TARGET => DO_NOTHING },
    )
    .build();

    let do_something_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NAME,
        runtime_args! { ARG_TARGET => DO_SOMETHING },
    )
    .build();

    let no_subcall_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NAME,
        runtime_args! { ARG_TARGET => NO_SUBCALL },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    builder.exec(do_nothing_request).expect_success().commit();

    builder.exec(do_something_request).expect_success().commit();

    builder.exec(no_subcall_request).expect_success().commit();

    let do_nothing_cost = builder.exec_costs(0)[0];

    let do_something_cost = builder.exec_costs(1)[0];

    let no_subcall_cost = builder.exec_costs(2)[0];

    assert_ne!(
        do_nothing_cost, do_something_cost,
        "should have different costs"
    );

    assert_ne!(
        no_subcall_cost, do_something_cost,
        "should have different costs"
    );

    assert!(
        do_nothing_cost < do_something_cost,
        "should cost more to do something via subcall"
    );

    assert!(
        no_subcall_cost < do_nothing_cost,
        "do nothing in a subcall should cost more than no subcall"
    );
}

#[ignore]
#[test]
fn should_add_all_gas_for_subcall() {
    const CONTRACT_NAME: &str = "add_gas_subcall.wasm";
    const ADD_GAS_FROM_SESSION: &str = "add-gas-from-session";
    const ADD_GAS_VIA_SUBCALL: &str = "add-gas-via-subcall";

    // Use 90% of the standard test contract's balance
    let gas_to_add: U512 = U512::from(u32::max_value());

    let gas_to_add_as_arg: u32 = gas_to_add.as_();

    let add_zero_gas_from_session_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NAME,
        runtime_args! {
            ARG_GAS_AMOUNT => 0,
            ARG_METHOD_NAME => ADD_GAS_FROM_SESSION,
        },
    )
    .build();

    let add_some_gas_from_session_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NAME,
        runtime_args! {
            ARG_GAS_AMOUNT => gas_to_add_as_arg,
            ARG_METHOD_NAME => ADD_GAS_FROM_SESSION,
        },
    )
    .build();

    let add_zero_gas_via_subcall_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NAME,
        runtime_args! {
            ARG_GAS_AMOUNT => 0,
            ARG_METHOD_NAME => ADD_GAS_VIA_SUBCALL,
        },
    )
    .build();

    let add_some_gas_via_subcall_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NAME,
        runtime_args! {
            ARG_GAS_AMOUNT => gas_to_add_as_arg,
            ARG_METHOD_NAME => ADD_GAS_VIA_SUBCALL,
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    builder
        .exec(add_zero_gas_from_session_request)
        .expect_success()
        .commit();
    builder
        .exec(add_some_gas_from_session_request)
        .expect_success()
        .commit();
    builder
        .exec(add_zero_gas_via_subcall_request)
        .expect_success()
        .commit();
    builder
        .exec(add_some_gas_via_subcall_request)
        .expect_success()
        .commit();

    let add_zero_gas_from_session_cost = builder.exec_costs(0)[0];
    let add_some_gas_from_session_cost = builder.exec_costs(1)[0];
    let add_zero_gas_via_subcall_cost = builder.exec_costs(2)[0];
    let add_some_gas_via_subcall_cost = builder.exec_costs(3)[0];

    assert!(add_some_gas_from_session_cost.value() > gas_to_add);
    assert_eq!(
        add_some_gas_from_session_cost.value(),
        gas_to_add + add_zero_gas_from_session_cost.value()
    );

    assert!(add_some_gas_via_subcall_cost.value() > gas_to_add);
    assert_eq!(
        add_some_gas_via_subcall_cost.value(),
        gas_to_add + add_zero_gas_via_subcall_cost.value()
    );
}

#[ignore]
#[test]
fn expensive_subcall_should_cost_more() {
    const DO_NOTHING: &str = "do_nothing_stored.wasm";
    const EXPENSIVE_CALCULATION: &str = "expensive_calculation.wasm";
    const DO_NOTHING_PACKAGE_HASH_KEY_NAME: &str = "do_nothing_package_hash";
    const EXPENSIVE_CALCULATION_KEY: &str = "expensive-calculation";
    const ENTRY_FUNCTION_NAME: &str = "delegate";

    let store_do_nothing_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, DO_NOTHING, RuntimeArgs::default())
            .build();

    let store_calculation_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        EXPENSIVE_CALCULATION,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    // store the contracts first
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    builder
        .exec(store_do_nothing_request)
        .expect_success()
        .commit();

    builder
        .exec(store_calculation_request)
        .expect_success()
        .commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");

    let expensive_calculation_contract_hash = account
        .named_keys()
        .get(EXPENSIVE_CALCULATION_KEY)
        .expect("should get expensive_calculation contract hash")
        .into_hash()
        .expect("should get hash");

    // execute the contracts via subcalls

    let call_do_nothing_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        DO_NOTHING_PACKAGE_HASH_KEY_NAME,
        Some(CONTRACT_INITIAL_VERSION),
        ENTRY_FUNCTION_NAME,
        RuntimeArgs::new(),
    )
    .build();

    let call_expensive_calculation_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        expensive_calculation_contract_hash.into(),
        "calculate",
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec(call_do_nothing_request)
        .expect_success()
        .commit();

    builder
        .exec(call_expensive_calculation_request)
        .expect_success()
        .commit();

    let do_nothing_cost = builder.exec_costs(2)[0];

    let expensive_calculation_cost = builder.exec_costs(3)[0];

    assert!(
        do_nothing_cost < expensive_calculation_cost,
        "calculation cost should be higher than doing nothing cost"
    );
}
