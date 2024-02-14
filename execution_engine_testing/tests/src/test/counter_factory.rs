use std::{collections::BTreeSet, iter::FromIterator};

use crate::wasm_utils;
use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution};
use casper_types::{
    addressable_entity::{EntityKindTag, DEFAULT_ENTRY_POINT_NAME},
    runtime_args, AddressableEntityHash, ByteCodeAddr, Key, RuntimeArgs, U512,
};

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
fn should_not_call_undefined_entrypoints_on_factory() {
    let (mut builder, contract_hash) = setup();

    let exec_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        DEFAULT_ENTRY_POINT_NAME, // should not be able to call "call" entry point
        RuntimeArgs::new(),
    )
    .build();

    builder.exec(exec_request_1).commit();

    let no_such_method_1 = builder.get_error().expect("should have error");

    assert!(
        matches!(no_such_method_1, Error::Exec(execution::Error::NoSuchMethod(function_name)) if function_name == DEFAULT_ENTRY_POINT_NAME)
    );

    // Can't call abstract entry point "increase" on the factory.

    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        INCREASE_ENTRY_POINT,
        RuntimeArgs::new(),
    )
    .build();

    builder.exec(exec_request_2).commit();

    let no_such_method_2 = builder.get_error().expect("should have error");

    assert!(
        matches!(&no_such_method_2, Error::Exec(execution::Error::TemplateMethod(function_name)) if function_name == INCREASE_ENTRY_POINT),
        "{:?}",
        &no_such_method_2
    );

    // Can't call abstract entry point "decrease" on the factory.

    let exec_request_3 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        DECREASE_ENTRY_POINT,
        RuntimeArgs::new(),
    )
    .build();

    builder.exec(exec_request_3).commit();

    let no_such_method_3 = builder.get_error().expect("should have error");

    assert!(
        matches!(&no_such_method_3, Error::Exec(execution::Error::TemplateMethod(function_name)) if function_name == DECREASE_ENTRY_POINT),
        "{:?}",
        &no_such_method_3
    );
}

#[ignore]
#[test]
fn contract_factory_wasm_should_have_expected_exports() {
    let (builder, contract_hash) = setup();

    let factory_contract_entity_key =
        Key::addressable_entity_key(EntityKindTag::SmartContract, contract_hash);

    let factory_contract = builder
        .query(None, factory_contract_entity_key, &[])
        .expect("should have contract")
        .as_addressable_entity()
        .cloned()
        .expect("should be contract");

    let factory_contract_byte_code_key = Key::byte_code_key(ByteCodeAddr::new_wasm_addr(
        factory_contract.byte_code_addr(),
    ));

    let factory_contract_wasm = builder
        .query(None, factory_contract_byte_code_key, &[])
        .expect("should have contract wasm")
        .as_byte_code()
        .cloned()
        .expect("should have wasm");

    let factory_wasm_exports = wasm_utils::get_wasm_exports(factory_contract_wasm.bytes());
    let expected_entrypoints = BTreeSet::from_iter([
        INCREASE_ENTRY_POINT.to_string(),
        DECREASE_ENTRY_POINT.to_string(),
        CONTRACT_FACTORY_ENTRY_POINT.to_string(),
        CONTRACT_FACTORY_DEFAULT_ENTRY_POINT.to_string(),
    ]);
    assert_eq!(factory_wasm_exports, expected_entrypoints);
}

#[ignore]
#[test]
fn should_install_and_use_factory_pattern() {
    let (mut builder, contract_hash) = setup();

    // Call a factory entrypoint
    let exec_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        CONTRACT_FACTORY_ENTRY_POINT,
        runtime_args! {
            ARG_NAME => NEW_COUNTER_1_NAME,
            ARG_INITIAL_VALUE => U512::one(),
        },
    )
    .build();

    builder.exec(exec_request_1).commit().expect_success();

    // Call a different factory entrypoint that accepts different set of arguments
    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        CONTRACT_FACTORY_DEFAULT_ENTRY_POINT,
        runtime_args! {
            ARG_NAME => NEW_COUNTER_2_NAME,
        },
    )
    .build();

    builder.exec(exec_request_2).commit().expect_success();

    let counter_factory_contract = builder
        .get_entity_with_named_keys_by_entity_hash(contract_hash)
        .expect("should have contract hash");

    let new_counter_1 = counter_factory_contract
        .named_keys()
        .get(NEW_COUNTER_1_NAME)
        .expect("new counter should exist")
        .into_entity_hash()
        .unwrap();

    let new_counter_1_contract = builder
        .get_addressable_entity(new_counter_1)
        .expect("should have contract instance");

    let new_counter_2 = counter_factory_contract
        .named_keys()
        .get(NEW_COUNTER_2_NAME)
        .expect("new counter should exist")
        .into_entity_hash()
        .unwrap();

    let _new_counter_2_contract = builder
        .get_addressable_entity(new_counter_2)
        .expect("should have contract instance");

    let counter_1_wasm = builder
        .query(
            None,
            Key::byte_code_key(ByteCodeAddr::new_wasm_addr(
                new_counter_1_contract.byte_code_addr(),
            )),
            &[],
        )
        .expect("should have contract wasm")
        .as_byte_code()
        .cloned()
        .expect("should have wasm");

    let new_counter_1_exports = wasm_utils::get_wasm_exports(counter_1_wasm.bytes());
    assert_eq!(
        new_counter_1_exports,
        BTreeSet::from_iter([
            INCREASE_ENTRY_POINT.to_string(),
            DECREASE_ENTRY_POINT.to_string()
        ])
    );

    let increment_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        new_counter_1,
        INCREASE_ENTRY_POINT,
        RuntimeArgs::new(),
    )
    .build();

    builder.exec(increment_request).commit().expect_success();

    let decrement_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        new_counter_1,
        DECREASE_ENTRY_POINT,
        RuntimeArgs::new(),
    )
    .build();

    builder.exec(decrement_request).commit().expect_success();
}

fn setup() -> (LmdbWasmTestBuilder, AddressableEntityHash) {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_COUNTER_FACTORY,
        RuntimeArgs::new(),
    )
    .build();

    builder.exec(exec_request).commit().expect_success();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have entity for account");

    let contract_hash_key = account
        .named_keys()
        .get("factory_hash")
        .expect("should have factory hash");

    (builder, contract_hash_key.into_entity_hash().unwrap())
}
