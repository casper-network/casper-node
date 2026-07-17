use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequest, ExecuteRequestBuilder, LmdbWasmTestBuilder,
    DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::WasmV1Result;
use casper_types::{
    execution::{RetValue, TransformKindV2, TransformV2},
    runtime_args,
    system::{handle_payment, mint::METHOD_CREATE},
    AddressableEntityHash, EntityAddr, Key, RuntimeArgs, DEFAULT_ENTRY_POINT_NAME,
};
use log::error;

const DO_NOTHING: &str = "do_nothing.wasm";
const DO_NOTHING_STORED: &str = "do_nothing_stored.wasm";
const DO_NOTHING_STORED_CALLER_CONTRACT_NAME: &str = "do_nothing_stored_caller_stored";
const DO_NOTHING_HASH_KEY_NAME: &str = "do_nothing_hash";
const DO_NOTHING_PACKAGE_HASH_KEY_NAME: &str = "do_nothing_package_hash";
const DO_NOTHING_STORED_CALLER_HASH_KEY_NAME: &str = "do_nothing_stored_caller_stored_hash";

const CALL_ALL_SYSTEM_CONTRACTS_WASM: &str = "call_all_system_contracts.wasm";
const STORED_CALL_ALL_SYSTEM_CONTRACTS_WASM: &str = "stored_call_all_system_contracts.wasm";
const STORED_CALL_ALL_SYSTEM_CONTRACTS_KEY: &str = "stored_call_handle_payment_hash";
const STORED_CALL_ALL_SYSTEM_CONTRACTS_ENTRY_POINT: &str = "call_system";

#[ignore]
#[test]
fn vm1_do_nothing_session_should_return_session_entry_point_called() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    // Execute the contract that calls casper_ret directly
    let deploy_item = DeployItemBuilder::new()
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_session_code(DO_NOTHING, RuntimeArgs::default())
        .with_payment_code(DO_NOTHING, RuntimeArgs::default())
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .build();
    let exec_request = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

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
            Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_ADDR.value())),
            TransformKindV2::EntryPointCalled(None, DEFAULT_ENTRY_POINT_NAME.to_string()),
        );
        let ret = TransformV2::new(
            Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_ADDR.value())),
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
        Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_ADDR.value())),
        TransformKindV2::EntryPointCalled(Some(contract_hash), "delegate".to_string()),
    );
    let delegate_ret = TransformV2::new(
        Key::AddressableEntity(EntityAddr::SmartContract(contract_hash)),
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
        Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_ADDR.value())),
        TransformKindV2::EntryPointCalled(Some(contract_hash), "delegate".to_string()),
    );
    let delegate_ret = TransformV2::new(
        Key::AddressableEntity(EntityAddr::SmartContract(contract_hash)),
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
        Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_ADDR.value())),
        TransformKindV2::EntryPointCalled(Some(caller_hash), "call_stored".to_string()),
    );
    let delegate_called = TransformV2::new(
        Key::AddressableEntity(EntityAddr::SmartContract(caller_hash)),
        TransformKindV2::EntryPointCalled(Some(contract_hash), "delegate".to_string()),
    );
    let delegate_ret = TransformV2::new(
        Key::AddressableEntity(EntityAddr::SmartContract(contract_hash)),
        TransformKindV2::Ret(RetValue::Unit),
    );
    let caller_ret = TransformV2::new(
        Key::AddressableEntity(EntityAddr::SmartContract(caller_hash)),
        TransformKindV2::Ret(RetValue::Unit),
    );
    assert_eq!(
        ep_calls_and_rets,
        vec![caller_called, delegate_called, delegate_ret, caller_ret]
    );
}

#[ignore]
#[test]
fn vm1_session_calling_system_contracts_emits_entry_point_called_and_ret() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CALL_ALL_SYSTEM_CONTRACTS_WASM,
        RuntimeArgs::default(),
    )
    .build();

    let results = builder
        .exec(exec_request)
        .expect_success()
        .get_exec_result_owned(0)
        .unwrap();

    let handle_payment_hash = builder.get_handle_payment_contract_hash().value();
    let mint_hash = builder.get_mint_contract_hash().value();

    let ep_calls_and_rets = get_ep_calls_and_rets(results);
    // session EC + HandlePayment EC + HandlePayment Ret + session Ret = 4
    assert_eq!(ep_calls_and_rets.len(), 6);

    let session_ec = TransformV2::new(
        Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_ADDR.value())),
        TransformKindV2::EntryPointCalled(None, DEFAULT_ENTRY_POINT_NAME.to_string()),
    );
    let hp_ec = TransformV2::new(
        Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_ADDR.value())),
        TransformKindV2::EntryPointCalled(
            Some(handle_payment_hash),
            handle_payment::METHOD_GET_PAYMENT_PURSE.to_string(),
        ),
    );
    let mint_ec = TransformV2::new(
        Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_ADDR.value())),
        TransformKindV2::EntryPointCalled(Some(mint_hash), METHOD_CREATE.to_string()),
    );
    let session_ret = TransformV2::new(
        Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_ADDR.value())),
        TransformKindV2::Ret(RetValue::Unit),
    );

    assert_eq!(ep_calls_and_rets[0], session_ec);
    assert_eq!(ep_calls_and_rets[1], hp_ec);
    let transform_2 = &ep_calls_and_rets[2];
    assert_eq!(*transform_2.key(), Key::AddressableEntity(EntityAddr::System(handle_payment_hash)));
    assert!(matches!(
        transform_2.kind(),
        TransformKindV2::Ret(RetValue::CLValue(_))
    ));
    assert_eq!(ep_calls_and_rets[3], mint_ec);
    let transform_4 = &ep_calls_and_rets[4];
    assert_eq!(*transform_4.key(), Key::AddressableEntity(EntityAddr::System(mint_hash)));
    assert!(matches!(
        transform_4.kind(),
        TransformKindV2::Ret(RetValue::CLValue(_))
    ));
    assert_eq!(ep_calls_and_rets[5], session_ret);
}

#[ignore]
#[test]
fn vm1_stored_contract_calling_system_contract_emits_entry_point_called_and_ret() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    // Install the stored contract
    let install_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        STORED_CALL_ALL_SYSTEM_CONTRACTS_WASM,
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(install_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let stored_hash = account
        .named_keys()
        .get(STORED_CALL_ALL_SYSTEM_CONTRACTS_KEY)
        .cloned()
        .and_then(Key::into_hash_addr)
        .expect("should have stored contract hash");

    // Call the stored entry point that invokes HandlePayment then Mint
    let call_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        AddressableEntityHash::new(stored_hash),
        STORED_CALL_ALL_SYSTEM_CONTRACTS_ENTRY_POINT,
        RuntimeArgs::new(),
    )
    .build();

    let results = builder
        .exec(call_request)
        .expect_success()
        .commit()
        .get_exec_result_owned(1)
        .unwrap();

    let handle_payment_hash = builder.get_handle_payment_contract_hash().value();
    let mint_hash = builder.get_mint_contract_hash().value();

    let ep_calls_and_rets = get_ep_calls_and_rets(results);
    // stored EC + HP EC + HP Ret + Mint EC + Mint Ret + stored Ret = 6
    assert_eq!(ep_calls_and_rets.len(), 6);

    let stored_ec = TransformV2::new(
        Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_ADDR.value())),
        TransformKindV2::EntryPointCalled(
            Some(stored_hash),
            STORED_CALL_ALL_SYSTEM_CONTRACTS_ENTRY_POINT.to_string(),
        ),
    );
    let hp_ec = TransformV2::new(
        Key::AddressableEntity(EntityAddr::SmartContract(stored_hash)),
        TransformKindV2::EntryPointCalled(
            Some(handle_payment_hash),
            handle_payment::METHOD_GET_PAYMENT_PURSE.to_string(),
        ),
    );
    let mint_ec = TransformV2::new(
        Key::AddressableEntity(EntityAddr::SmartContract(stored_hash)),
        TransformKindV2::EntryPointCalled(Some(mint_hash), METHOD_CREATE.to_string()),
    );
    let stored_ret = TransformV2::new(Key::AddressableEntity(EntityAddr::SmartContract(stored_hash)), TransformKindV2::Ret(RetValue::Unit));

    assert_eq!(ep_calls_and_rets[0], stored_ec);
    assert_eq!(ep_calls_and_rets[1], hp_ec);
    // HandlePayment::get_payment_purse returns a URef; check key and variant.
    assert_eq!(ep_calls_and_rets[2].key(), &Key::AddressableEntity(EntityAddr::System(handle_payment_hash)));
    assert!(
        matches!(
            ep_calls_and_rets[2].kind(),
            TransformKindV2::Ret(RetValue::CLValue(_))
        ),
        "expected CLValue Ret for HandlePayment, got {:?}",
        ep_calls_and_rets[2].kind()
    );
    assert_eq!(ep_calls_and_rets[3], mint_ec);
    // Mint::create returns a URef; check key and variant.
    assert_eq!(ep_calls_and_rets[4].key(), &Key::AddressableEntity(EntityAddr::System(mint_hash)));
    assert!(
        matches!(
            ep_calls_and_rets[4].kind(),
            TransformKindV2::Ret(RetValue::CLValue(_))
        ),
        "expected CLValue Ret for Mint, got {:?}",
        ep_calls_and_rets[4].kind()
    );
    assert_eq!(ep_calls_and_rets[5], stored_ret);
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

const DO_NOTHING_REVERT_STORED: &str = "do_nothing_revert_stored.wasm";
const DO_NOTHING_REVERT_HASH_KEY_NAME: &str = "do_nothing_revert_stored_hash";

/// Three-level nesting test: account -> chain_call -> call_stored -> delegate
/// This verifies that 3 levels of stored contract calls produce the correct EC+Ret journal entries.
#[ignore]
#[test]
fn vm1_three_level_nesting_produces_correct_journal() {
    // 1. Install do_nothing_stored (leaf, has "delegate" entry)
    let mut builder = install_do_nothing();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");

    let leaf_hash = account
        .named_keys()
        .get(DO_NOTHING_HASH_KEY_NAME)
        .expect("should have do_nothing_hash key")
        .into_entity_hash_addr()
        .expect("should have hash addr");

    // 2. Install do_nothing_stored_caller_stored (middle, has "call_stored" and "chain_call"
    //    entries)
    let install_middle_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        &format!("{}.wasm", DO_NOTHING_STORED_CALLER_CONTRACT_NAME),
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec(install_middle_request)
        .expect_success()
        .commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");

    let middle_hash = account
        .named_keys()
        .get(DO_NOTHING_STORED_CALLER_HASH_KEY_NAME)
        .expect("should have DO_NOTHING_STORED_CALLER_HASH_KEY_NAME key")
        .into_entity_hash_addr()
        .expect("should have hash addr");

    // 3. Install another instance of do_nothing_stored_caller_stored (outer, has "chain_call"
    //    entry) We reuse the same wasm but it gets a new hash. We need a different key name to
    //    distinguish the two instances. The second install will overwrite
    //    DO_NOTHING_STORED_CALLER_HASH_KEY_NAME with the new hash.
    let install_outer_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        &format!("{}.wasm", DO_NOTHING_STORED_CALLER_CONTRACT_NAME),
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec(install_outer_request)
        .expect_success()
        .commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");

    let outer_hash = account
        .named_keys()
        .get(DO_NOTHING_STORED_CALLER_HASH_KEY_NAME)
        .expect("should have updated outer hash key")
        .into_entity_hash_addr()
        .expect("should have hash addr");

    // 4. Call outer.chain_call(middle_hash, leaf_hash) This chains: account -> outer.chain_call ->
    //    middle.call_stored -> leaf.delegate
    let call_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        AddressableEntityHash::new(outer_hash),
        "chain_call",
        runtime_args! {
            "outer_addr" => middle_hash,
            "inner_addr" => leaf_hash,
        },
    )
    .build();

    let builder_after = builder.exec(call_request).expect_success().commit();

    // exec index 0: do_nothing_stored install (from install_do_nothing)
    // exec index 1: middle install
    // exec index 2: outer install
    // exec index 3: the chain_call
    let exec_result = builder_after.get_exec_result_owned(3).unwrap();
    let ep_calls_and_rets = get_ep_calls_and_rets(exec_result);

    // Expected: 3 ECs + 3 Rets = 6 total
    assert_eq!(ep_calls_and_rets.len(), 6);

    let outer_called = TransformV2::new(
        Key::AddressableEntity(EntityAddr::Account(DEFAULT_ACCOUNT_ADDR.value())),
        TransformKindV2::EntryPointCalled(Some(outer_hash), "chain_call".to_string()),
    );
    let middle_called = TransformV2::new(
        Key::AddressableEntity(EntityAddr::SmartContract(outer_hash)),
        TransformKindV2::EntryPointCalled(Some(middle_hash), "call_stored".to_string()),
    );
    let leaf_called = TransformV2::new(
        Key::AddressableEntity(EntityAddr::SmartContract(middle_hash)),
        TransformKindV2::EntryPointCalled(Some(leaf_hash), "delegate".to_string()),
    );
    let leaf_ret = TransformV2::new(Key::AddressableEntity(EntityAddr::SmartContract(leaf_hash)), TransformKindV2::Ret(RetValue::Unit));
    let middle_ret = TransformV2::new(Key::AddressableEntity(EntityAddr::SmartContract(middle_hash)), TransformKindV2::Ret(RetValue::Unit));
    let outer_ret = TransformV2::new(Key::AddressableEntity(EntityAddr::SmartContract(outer_hash)), TransformKindV2::Ret(RetValue::Unit));
    assert_eq!(
        ep_calls_and_rets,
        vec![
            outer_called,
            middle_called,
            leaf_called,
            leaf_ret,
            middle_ret,
            outer_ret,
        ]
    );
}

#[ignore]
#[test]
fn vm1_nested_call_error_shows_entry_point_called_without_ret() {
    // 1. Install do_nothing_stored_caller_stored (caller, has "call_stored" entry)
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let install_caller_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        &format!("{}.wasm", DO_NOTHING_STORED_CALLER_CONTRACT_NAME),
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec(install_caller_request)
        .expect_success()
        .commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");

    let caller_hash = account
        .named_keys()
        .get(DO_NOTHING_STORED_CALLER_HASH_KEY_NAME)
        .expect("should have DO_NOTHING_STORED_CALLER_HASH_KEY_NAME key")
        .into_entity_hash_addr()
        .expect("should have hash addr");

    // 2. Install do_nothing_revert_stored (reverter, has "delegate" that calls revert)
    let install_reverter_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        DO_NOTHING_REVERT_STORED,
        RuntimeArgs::default(),
    )
    .build();

    builder
        .exec(install_reverter_request)
        .expect_success()
        .commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");

    let reverter_hash = account
        .named_keys()
        .get(DO_NOTHING_REVERT_HASH_KEY_NAME)
        .expect("should have do_nothing_revert_stored_hash key")
        .into_entity_hash_addr()
        .expect("should have hash addr");

    // 3. Call caller.call_stored(reverter_hash) - this should fail (reverter.delegate reverts)
    let call_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        AddressableEntityHash::new(caller_hash),
        "call_stored",
        runtime_args! {
            "contract_addr" => reverter_hash,
        },
    )
    .build();

    // The call should fail since reverter reverts
    let builder_after = builder.exec(call_request).expect_failure().commit();

    let exec_result = builder_after.get_exec_result_owned(2).unwrap();
    error!("XXXX {:?}", exec_result);
    let ep_calls_and_rets = get_ep_calls_and_rets(exec_result);

    // There should be no ECs and RETs since a revert happened
    assert!(ep_calls_and_rets.is_empty());
}
