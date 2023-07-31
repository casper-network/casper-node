use std::{collections::BTreeSet, iter::FromIterator};

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution};
use casper_types::{contracts::DEFAULT_ENTRY_POINT_NAME, runtime_args, RuntimeArgs, U512};
use walrus::Module;

const CONTRACT_COUNTER_FACTORY: &str = "counter_factory.wasm";
const CONTRACT_FACTORY_DEFAULT_ENTRY_POINT: &str = "contract_factory_default";
const CONTRACT_FACTORY_ENTRY_POINT: &str = "contract_factory";
const DECREASE_ENTRY_POINT: &str = "decrement";
const INCREASE_ENTRY_POINT: &str = "increment";
const ARG_INITIAL_VALUE: &str = "initial_value";
const ARG_NAME: &str = "name";
const NEW_COUNTER_1_NAME: &str = "new-counter-1";
const NEW_COUNTER_2_NAME: &str = "new-counter-2";

#[ignore]
#[test]
fn should_install_factory() {
    let block_time: u64 = 42;

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_COUNTER_FACTORY,
        RuntimeArgs::new(),
    )
    .with_block_time(block_time)
    .build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    builder.exec(exec_request).commit().expect_success();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let contract_hash_key = account
        .named_keys()
        .get("factory_hash")
        .expect("should have factory hash");
    let factory_contract = builder
        .query(None, *contract_hash_key, &[])
        .expect("should have contract")
        .as_contract()
        .cloned()
        .expect("should be contract");
    let factory_contract_wasm = builder
        .query(None, factory_contract.contract_wasm_key(), &[])
        .expect("should have contract wasm")
        .as_contract_wasm()
        .cloned()
        .expect("should have wasm");

    let module = Module::from_buffer(factory_contract_wasm.bytes())
        .expect("should have valid wasm bytes stored in the global state");
    let factory_wasm_exports: BTreeSet<&str> = module
        .exports
        .iter()
        .map(|export| export.name.as_str())
        .collect();

    let expected_entrypoints = BTreeSet::from_iter([
        INCREASE_ENTRY_POINT,
        DECREASE_ENTRY_POINT,
        CONTRACT_FACTORY_ENTRY_POINT,
        CONTRACT_FACTORY_DEFAULT_ENTRY_POINT,
    ]);
    assert_eq!(factory_wasm_exports, expected_entrypoints);

    let contract_hash = (*contract_hash_key).into_contract_hash().unwrap();
    let exec_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        "call", // should not be able to call "call" entry point
        RuntimeArgs::new(),
    )
    .with_block_time(block_time)
    .build();

    builder.exec(exec_request_1).commit();

    let no_such_method = builder.get_error().expect("should have error");

    assert!(
        matches!(no_such_method, Error::Exec(execution::Error::NoSuchMethod(function_name)) if function_name == DEFAULT_ENTRY_POINT_NAME)
    );

    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        INCREASE_ENTRY_POINT,
        RuntimeArgs::new(),
    )
    .with_block_time(block_time)
    .build();

    builder.exec(exec_request_2).commit();

    let no_such_method = builder.get_error().expect("should have error");

    assert!(
        matches!(&no_such_method, Error::Exec(execution::Error::NoSuchMethod(function_name)) if function_name == INCREASE_ENTRY_POINT),
        "{:?}",
        &no_such_method
    );

    let exec_request_3 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        CONTRACT_FACTORY_ENTRY_POINT,
        runtime_args! {
            ARG_NAME => NEW_COUNTER_1_NAME,
            ARG_INITIAL_VALUE => U512::one(),
        },
    )
    .with_block_time(block_time)
    .build();

    builder.exec(exec_request_3).commit().expect_success();

    let exec_request_4 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        CONTRACT_FACTORY_DEFAULT_ENTRY_POINT,
        runtime_args! {
            ARG_NAME => NEW_COUNTER_2_NAME,
        },
    )
    .with_block_time(block_time)
    .build();

    builder.exec(exec_request_4).commit().expect_success();

    let new_counter_1_contract = builder
        .get_contract(contract_hash)
        .expect("should have contract hash");

    let new_counter_1 = new_counter_1_contract
        .named_keys()
        .get(NEW_COUNTER_1_NAME)
        .expect("new counter should exist")
        .into_contract_hash()
        .unwrap();

    let new_counter_2 = new_counter_1_contract
        .named_keys()
        .get(NEW_COUNTER_2_NAME)
        .expect("new counter should exist")
        .into_contract_hash()
        .unwrap();

    let new_counter_1_contract = builder
        .get_contract(new_counter_1)
        .expect("should have contract instance");

    let _new_counter_2_contract = builder
        .get_contract(new_counter_2)
        .expect("should have contract instance");

    let contract_wasm = builder
        .query(None, new_counter_1_contract.contract_wasm_key(), &[])
        .expect("should have contract wasm")
        .as_contract_wasm()
        .cloned()
        .expect("should have wasm");

    let new_counter_1_module =
        Module::from_buffer(contract_wasm.bytes()).expect("should be valid wasm bytes");
    assert_eq!(
        new_counter_1_module
            .exports
            .iter()
            .map(|export| export.name.as_str())
            .collect::<BTreeSet<&str>>(),
        BTreeSet::from_iter([INCREASE_ENTRY_POINT, DECREASE_ENTRY_POINT])
    );

    let increment_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        new_counter_1,
        INCREASE_ENTRY_POINT,
        RuntimeArgs::new(),
    )
    .with_block_time(block_time)
    .build();

    builder.exec(increment_request).commit().expect_success();

    let decrement_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        new_counter_1,
        DECREASE_ENTRY_POINT,
        RuntimeArgs::new(),
    )
    .with_block_time(block_time)
    .build();

    builder.exec(decrement_request).commit().expect_success();
}
