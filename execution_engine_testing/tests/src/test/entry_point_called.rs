use casper_engine_test_support::{
    ExecuteRequest, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::WasmV1Result;
use casper_types::{
    execution::{RetValue, TransformKindV2, TransformV2},
    runtime_args, AddressableEntityHash, Key, RuntimeArgs, DEFAULT_ENTRY_POINT_NAME,
};

const DO_NOTHING: &str = "do_nothing.wasm";
const DO_NOTHING_STORED: &str = "do_nothing_stored.wasm";
const DO_NOTHING_STORED_CALLER_CONTRACT_NAME: &str = "do_nothing_stored_caller_stored";
const DO_NOTHING_HASH_KEY_NAME: &str = "do_nothing_hash";
const DO_NOTHING_PACKAGE_HASH_KEY_NAME: &str = "do_nothing_package_hash";
const DO_NOTHING_STORED_CALLER_HASH_KEY_NAME: &str = "do_nothing_stored_caller_stored_hash";

#[ignore]
#[test]
fn vm1_do_nothing_session_should_not_return_entry_point_called() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    // Execute the contract that calls casper_ret directly
    let exec_request = ExecuteRequestBuilder::standard_and_payment(
        *DEFAULT_ACCOUNT_ADDR,
        DO_NOTHING,
        RuntimeArgs::default(),
        DO_NOTHING,
        RuntimeArgs::default(),
    )
    .build();

    let builder_after_success = builder.exec(exec_request).expect_success();
    assert_eq!(builder_after_success.get_exec_results_count(), 2);
    for i in 0..=1 {
        let results = builder_after_success
            .get_exec_result_owned(i)
            .clone()
            .unwrap();
        let ep_calls_and_rets = get_ep_calls_and_rets(results);
        assert_eq!(ep_calls_and_rets.len(), 2);
        let call_called = TransformV2::new(
            Key::Account(*DEFAULT_ACCOUNT_ADDR),
            TransformKindV2::EntryPointCalled(None, DEFAULT_ENTRY_POINT_NAME.to_string()),
        );
        let ret = TransformV2::new(
            Key::Account(*DEFAULT_ACCOUNT_ADDR),
            TransformKindV2::Ret(RetValue::Unit),
        );
        assert_eq!(ep_calls_and_rets, vec![call_called, ret]);
    }
}

#[ignore]
#[test]
fn call_and_ret_when_by_hash() {
    run_test_do_nothing_call(|_contract_hash, package_hash| {
        ExecuteRequestBuilder::contract_call_by_hash_versioned_with_major(
            *DEFAULT_ACCOUNT_ADDR,
            package_hash.into(),
            None,
            None,
            "delegate",
            RuntimeArgs::new(),
        )
        .build()
    });
    run_test_do_nothing_call(|_contract_hash, package_hash| {
        ExecuteRequestBuilder::contract_call_by_hash_versioned_with_major(
            *DEFAULT_ACCOUNT_ADDR,
            package_hash.into(),
            None,
            Some(2),
            "delegate",
            RuntimeArgs::new(),
        )
        .build()
    });
    run_test_do_nothing_call(|_contract_hash, package_hash| {
        ExecuteRequestBuilder::contract_call_by_hash_versioned_with_major(
            *DEFAULT_ACCOUNT_ADDR,
            package_hash.into(),
            Some(1),
            None,
            "delegate",
            RuntimeArgs::new(),
        )
        .build()
    });
    run_test_do_nothing_call(|_contract_hash, package_hash| {
        ExecuteRequestBuilder::contract_call_by_hash_versioned_with_major(
            *DEFAULT_ACCOUNT_ADDR,
            package_hash.into(),
            Some(1),
            Some(2),
            "delegate",
            RuntimeArgs::new(),
        )
        .build()
    });
    run_test_do_nothing_call(|_, _| {
        ExecuteRequestBuilder::contract_call_by_name_versioned_with_major(
            *DEFAULT_ACCOUNT_ADDR,
            DO_NOTHING_PACKAGE_HASH_KEY_NAME,
            None,
            None,
            "delegate",
            RuntimeArgs::new(),
        )
        .build()
    });
    run_test_do_nothing_call(|_, _| {
        ExecuteRequestBuilder::contract_call_by_name_versioned_with_major(
            *DEFAULT_ACCOUNT_ADDR,
            DO_NOTHING_PACKAGE_HASH_KEY_NAME,
            None,
            Some(2),
            "delegate",
            RuntimeArgs::new(),
        )
        .build()
    });
    run_test_do_nothing_call(|_, _| {
        ExecuteRequestBuilder::contract_call_by_name_versioned_with_major(
            *DEFAULT_ACCOUNT_ADDR,
            DO_NOTHING_PACKAGE_HASH_KEY_NAME,
            Some(1),
            None,
            "delegate",
            RuntimeArgs::new(),
        )
        .build()
    });
    run_test_do_nothing_call(|_, _| {
        ExecuteRequestBuilder::contract_call_by_name_versioned_with_major(
            *DEFAULT_ACCOUNT_ADDR,
            DO_NOTHING_PACKAGE_HASH_KEY_NAME,
            Some(1),
            Some(2),
            "delegate",
            RuntimeArgs::new(),
        )
        .build()
    });
}

fn run_test_do_nothing_call<F>(build_request: F)
where
    F: FnOnce([u8; 32], [u8; 32]) -> ExecuteRequest,
{
    let mut builder = install_do_nothing();

    let default_account_entity = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let contract_hash = default_account_entity
        .named_keys()
        .get(DO_NOTHING_HASH_KEY_NAME)
        .cloned()
        .and_then(Key::into_hash_addr)
        .expect("should have hash");

    let package_hash = default_account_entity
        .named_keys()
        .get(DO_NOTHING_PACKAGE_HASH_KEY_NAME)
        .cloned()
        .and_then(Key::into_hash_addr)
        .expect("should have package hash");

    let request = build_request(contract_hash, package_hash);

    let results = builder
        .exec(request)
        .expect_success()
        .commit()
        .get_exec_result_owned(1)
        .unwrap();

    let ep_calls_and_rets = get_ep_calls_and_rets(results);
    assert_eq!(ep_calls_and_rets.len(), 2);
    let delegate_called = TransformV2::new(
        Key::Account(*DEFAULT_ACCOUNT_ADDR),
        TransformKindV2::EntryPointCalled(Some(contract_hash), "delegate".to_string()),
    );
    let delegate_ret = TransformV2::new(
        Key::Account(*DEFAULT_ACCOUNT_ADDR),
        TransformKindV2::Ret(RetValue::Unit),
    );
    assert_eq!(ep_calls_and_rets, vec![delegate_called, delegate_ret]);
}

#[ignore]
#[test]
fn vm1_do_nothing_stored_should_return_entry_point_called() {
    let mut builder = install_do_nothing();

    let default_account_entity = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");
    let contract_hash = default_account_entity
        .named_keys()
        .get(DO_NOTHING_HASH_KEY_NAME)
        .cloned()
        .and_then(Key::into_hash_addr)
        .expect("should have hash");

    let call_do_nothing_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        AddressableEntityHash::new(contract_hash),
        "delegate",
        RuntimeArgs::new(),
    )
    .build();

    let results = builder
        .exec(call_do_nothing_request)
        .expect_success()
        .commit()
        .get_exec_result_owned(1)
        .unwrap();

    let ep_calls_and_rets = get_ep_calls_and_rets(results);
    assert_eq!(ep_calls_and_rets.len(), 2);
    let delegate_called = TransformV2::new(
        Key::Account(*DEFAULT_ACCOUNT_ADDR),
        TransformKindV2::EntryPointCalled(Some(contract_hash), "delegate".to_string()),
    );
    let delegate_ret = TransformV2::new(
        Key::Account(*DEFAULT_ACCOUNT_ADDR),
        TransformKindV2::Ret(RetValue::Unit),
    );
    assert_eq!(ep_calls_and_rets, vec![delegate_called, delegate_ret]);
}

#[ignore]
#[test]
fn vm1_nested_call_should_produce_entry_point_calls_and_rets() {
    // 1. install do nothing stored contract
    let mut builder = install_do_nothing();
    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");

    let contract_hash = account
        .named_keys()
        .get(DO_NOTHING_HASH_KEY_NAME)
        .expect("should have key of do_nothing_hash")
        .into_entity_hash_addr()
        .expect("should have into hash");

    // 2. install caller
    let install_do_nothing_caller_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        &format!("{}.wasm", DO_NOTHING_STORED_CALLER_CONTRACT_NAME),
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec(install_do_nothing_caller_request)
        .expect_success()
        .commit();
    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");
    let caller_hash = account
        .named_keys()
        .get(DO_NOTHING_STORED_CALLER_HASH_KEY_NAME)
        .expect("should have key of DO_NOTHING_STORED_CALLER_HASH_KEY_NAME")
        .into_entity_hash_addr()
        .expect("should have into hash");

    // 3. Call the caller
    let call_do_nothing_caller_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        AddressableEntityHash::new(caller_hash),
        "call_stored",
        runtime_args! {
            "contract_addr" => contract_hash
        },
    )
    .build();

    let builder_after_success = builder
        .exec(call_do_nothing_caller_request)
        .expect_success()
        .commit();
    assert_eq!(builder_after_success.get_exec_results_count(), 3);
    let exec_owned = builder_after_success.get_exec_result_owned(2).unwrap();
    let ep_calls_and_rets = get_ep_calls_and_rets(exec_owned);
    assert_eq!(ep_calls_and_rets.len(), 4);
    let caller_called = TransformV2::new(
        Key::Account(*DEFAULT_ACCOUNT_ADDR),
        TransformKindV2::EntryPointCalled(Some(caller_hash), "call_stored".to_string()),
    );
    let delegate_called = TransformV2::new(
        Key::Hash(caller_hash),
        TransformKindV2::EntryPointCalled(Some(contract_hash), "delegate".to_string()),
    );
    let delegate_ret =
        TransformV2::new(Key::Hash(caller_hash), TransformKindV2::Ret(RetValue::Unit));
    let caller_ret = TransformV2::new(
        Key::Account(*DEFAULT_ACCOUNT_ADDR),
        TransformKindV2::Ret(RetValue::Unit),
    );
    assert_eq!(
        ep_calls_and_rets,
        vec![caller_called, delegate_called, delegate_ret, caller_ret]
    );
}

fn get_ep_calls_and_rets(results: WasmV1Result) -> Vec<TransformV2> {
    let transforms = results.effects().transforms();
    transforms
        .iter()
        .filter(|transform| {
            matches!(
                transform.kind(),
                casper_types::execution::TransformKindV2::EntryPointCalled(_, _)
            ) || matches!(
                transform.kind(),
                casper_types::execution::TransformKindV2::Ret(_)
            )
        })
        .cloned()
        .collect()
}

fn install_do_nothing() -> casper_engine_test_support::WasmTestBuilder<
    casper_storage::data_access_layer::DataAccessLayer<
        casper_storage::global_state::state::lmdb::LmdbGlobalState,
    >,
> {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());
    let install_do_nothing_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        DO_NOTHING_STORED,
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec(install_do_nothing_request)
        .expect_success()
        .commit();
    builder
}
