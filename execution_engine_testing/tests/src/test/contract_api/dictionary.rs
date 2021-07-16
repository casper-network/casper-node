use casper_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    AccountHash, DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::core::{engine_state::Error as EngineError, execution::Error};
use casper_types::{
    runtime_args, system::mint, AccessRights, ApiError, CLType, ContractHash, Key, RuntimeArgs,
    U512,
};

use dictionary_call::{NEW_DICTIONARY_ITEM_KEY, NEW_DICTIONARY_VALUE};

const DICTIONARY_WASM: &str = "dictionary.wasm";
const DICTIONARY_CALL_WASM: &str = "dictionary_call.wasm";
const DICTIONARY_ITEM_KEY_CHECK: &str = "dictionary-item-key-check.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);

fn setup() -> (InMemoryWasmTestBuilder, ContractHash) {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let fund_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => ACCOUNT_1_ADDR,
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    let install_contract_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        DICTIONARY_WASM,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(fund_request).commit().expect_success();

    builder
        .exec(install_contract_request)
        .commit()
        .expect_success();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    assert!(account
        .named_keys()
        .contains_key(dictionary::MALICIOUS_KEY_NAME));
    assert!(account
        .named_keys()
        .contains_key(dictionary::DICTIONARY_REF));

    let contract_hash = account
        .named_keys()
        .get(dictionary::CONTRACT_HASH_NAME)
        .cloned()
        .and_then(Key::into_hash)
        .map(ContractHash::new)
        .expect("should have hash");

    (builder, contract_hash)
}

#[ignore]
#[test]
fn should_modify_with_owned_access_rights() {
    let (mut builder, contract_hash) = setup();

    let modify_write_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        dictionary::MODIFY_WRITE_ENTRYPOINT,
        RuntimeArgs::default(),
    )
    .build();
    let modify_write_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        dictionary::MODIFY_WRITE_ENTRYPOINT,
        RuntimeArgs::default(),
    )
    .build();

    let contract = builder
        .get_contract(contract_hash)
        .expect("should have account");

    let stored_dictionary_key = contract
        .named_keys()
        .get(dictionary::DICTIONARY_NAME)
        .expect("dictionary");
    let dictionary_seed_uref = stored_dictionary_key.into_uref().expect("should be uref");

    let key_bytes = dictionary::DICTIONARY_PUT_KEY.as_bytes();
    let dictionary_key = Key::dictionary(dictionary_seed_uref, key_bytes);

    builder
        .exec(modify_write_request_1)
        .commit()
        .expect_success();

    let stored_value = builder
        .query(None, dictionary_seed_uref.into(), &[])
        .expect("should have value");
    let dictionary_uref_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should have cl value");
    assert_eq!(
        dictionary_uref_value.cl_type(),
        &CLType::Unit,
        "created dictionary uref should be unit"
    );

    let stored_value = builder
        .query(None, dictionary_key, &[])
        .expect("should have value");
    let dictionary_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should have cl value");

    let s: String = dictionary_value.into_t().expect("should be a string");
    assert_eq!(s, "Hello, world!");

    builder
        .exec(modify_write_request_2)
        .commit()
        .expect_success();

    let stored_value = builder
        .query(None, dictionary_key, &[])
        .expect("should have value");
    let dictionary_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should have cl value");

    let s: String = dictionary_value.into_t().expect("should be a string");
    assert_eq!(s, "Hello, world! Hello, world!");
}

#[ignore]
#[test]
fn should_not_write_with_read_access_rights() {
    let (mut builder, contract_hash) = setup();

    let call_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        DICTIONARY_CALL_WASM,
        runtime_args! {
            dictionary_call::ARG_OPERATION => dictionary_call::OP_WRITE,
            dictionary_call::ARG_SHARE_UREF_ENTRYPOINT => dictionary::SHARE_RO_ENTRYPOINT,
            dictionary_call::ARG_CONTRACT_HASH => contract_hash,
        },
    )
    .build();

    builder.exec(call_request).commit();

    let exec_results = builder
        .get_exec_results()
        .last()
        .expect("should have results");
    assert_eq!(exec_results.len(), 1);
    let error = exec_results[0].as_error().expect("should have error");
    assert!(
        matches!(
            error,
            EngineError::Exec(Error::InvalidAccess {
                required: AccessRights::WRITE
            })
        ),
        "Received error {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_read_with_read_access_rights() {
    let (mut builder, contract_hash) = setup();

    let call_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        DICTIONARY_CALL_WASM,
        runtime_args! {
            dictionary_call::ARG_OPERATION => dictionary_call::OP_READ,
            dictionary_call::ARG_SHARE_UREF_ENTRYPOINT => dictionary::SHARE_RO_ENTRYPOINT,
            dictionary_call::ARG_CONTRACT_HASH => contract_hash,
        },
    )
    .build();

    builder.exec(call_request).expect_success().commit();
}

#[ignore]
#[test]
fn should_not_read_with_write_access_rights() {
    let (mut builder, contract_hash) = setup();

    let call_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        DICTIONARY_CALL_WASM,
        runtime_args! {
            dictionary_call::ARG_OPERATION => dictionary_call::OP_READ,
            dictionary_call::ARG_SHARE_UREF_ENTRYPOINT => dictionary::SHARE_W_ENTRYPOINT,
            dictionary_call::ARG_CONTRACT_HASH => contract_hash,
        },
    )
    .build();

    builder.exec(call_request).commit();

    let exec_results = builder
        .get_exec_results()
        .last()
        .expect("should have results");
    assert_eq!(exec_results.len(), 1);
    let error = exec_results[0].as_error().expect("should have error");
    assert!(
        matches!(
            error,
            EngineError::Exec(Error::InvalidAccess {
                required: AccessRights::READ
            })
        ),
        "Received error {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_write_with_write_access_rights() {
    let (mut builder, contract_hash) = setup();

    let call_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        DICTIONARY_CALL_WASM,
        runtime_args! {
            dictionary_call::ARG_OPERATION => dictionary_call::OP_WRITE,
            dictionary_call::ARG_SHARE_UREF_ENTRYPOINT => dictionary::SHARE_W_ENTRYPOINT,
            dictionary_call::ARG_CONTRACT_HASH => contract_hash,
        },
    )
    .build();

    builder.exec(call_request).commit();

    let contract = builder
        .get_contract(contract_hash)
        .expect("should have account");

    let stored_dictionary_key = contract
        .named_keys()
        .get(dictionary::DICTIONARY_NAME)
        .expect("dictionary");
    let dictionary_root_uref = stored_dictionary_key.into_uref().expect("should be uref");

    let dictionary_key = Key::dictionary(dictionary_root_uref, NEW_DICTIONARY_ITEM_KEY.as_bytes());

    let result = builder
        .query(None, dictionary_key, &[])
        .expect("should query");
    let cl_value = result.as_cl_value().cloned().expect("should have cl value");
    let written_value: String = cl_value.into_t().expect("should get string");
    assert_eq!(written_value, NEW_DICTIONARY_VALUE);
}

#[ignore]
#[test]
fn should_not_write_with_forged_uref() {
    let (mut builder, contract_hash) = setup();

    let contract = builder
        .get_contract(contract_hash)
        .expect("should have account");

    let stored_dictionary_key = contract
        .named_keys()
        .get(dictionary::DICTIONARY_NAME)
        .expect("dictionary");
    let dictionary_root_uref = stored_dictionary_key.into_uref().expect("should be uref");

    // Do some extra forging on the uref
    let forged_uref = dictionary_root_uref.into_read_add_write();

    let call_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        DICTIONARY_CALL_WASM,
        runtime_args! {
            dictionary_call::ARG_OPERATION => dictionary_call::OP_FORGED_UREF_WRITE,
            dictionary_call::ARG_FORGED_UREF => forged_uref,
        },
    )
    .build();

    builder.exec(call_request).commit();

    let exec_results = builder
        .get_exec_results()
        .last()
        .expect("should have results");
    assert_eq!(exec_results.len(), 1);
    let error = exec_results[0].as_error().expect("should have error");
    assert!(
        matches!(
            error,
            EngineError::Exec(Error::ForgedReference(uref))
            if *uref == forged_uref
        ),
        "Received error {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_fail_put_with_invalid_dictionary_item_key() {
    let (mut builder, contract_hash) = setup();
    let contract = builder
        .get_contract(contract_hash)
        .expect("should have account");

    let _stored_dictionary_key = contract
        .named_keys()
        .get(dictionary::DICTIONARY_NAME)
        .expect("dictionary");

    let call_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        DICTIONARY_CALL_WASM,
        runtime_args! {
            dictionary_call::ARG_OPERATION => dictionary_call::OP_INVALID_PUT_DICTIONARY_ITEM_KEY,
            dictionary_call::ARG_CONTRACT_HASH => contract_hash
        },
    )
    .build();

    builder.exec(call_request).commit();
    let exec_results = builder
        .get_exec_results()
        .last()
        .expect("should have results");
    assert_eq!(exec_results.len(), 1);
    let error = exec_results[0].as_error().expect("should have error");
    assert!(
        matches!(
            error,
            EngineError::Exec(Error::Revert(ApiError::InvalidDictionaryItemKey))
        ),
        "Received error {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_fail_get_with_invalid_dictionary_item_key() {
    let (mut builder, contract_hash) = setup();
    let contract = builder
        .get_contract(contract_hash)
        .expect("should have account");

    let _stored_dictionary_key = contract
        .named_keys()
        .get(dictionary::DICTIONARY_NAME)
        .expect("dictionary");

    let call_request = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        DICTIONARY_CALL_WASM,
        runtime_args! {
            dictionary_call::ARG_OPERATION => dictionary_call::OP_INVALID_GET_DICTIONARY_ITEM_KEY,
            dictionary_call::ARG_CONTRACT_HASH => contract_hash
        },
    )
    .build();

    builder.exec(call_request).commit();
    let exec_results = builder
        .get_exec_results()
        .last()
        .expect("should have results");
    assert_eq!(exec_results.len(), 1);
    let error = exec_results[0].as_error().expect("should have error");
    assert!(
        matches!(
            error,
            EngineError::Exec(Error::Revert(ApiError::InvalidDictionaryItemKey))
        ),
        "Received error {:?}",
        error
    );
}

#[ignore]
#[test]
fn dictionary_put_should_fail_with_large_item_key() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let fund_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => ACCOUNT_1_ADDR,
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    let install_contract_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        DICTIONARY_ITEM_KEY_CHECK,
        runtime_args! {
            "dictionary-operation" => "put"
        },
    )
    .build();

    builder.exec(fund_request).commit().expect_success();
    builder.exec(install_contract_request).commit();
    let exec_results = builder
        .get_exec_results()
        .last()
        .expect("should have results");
    assert_eq!(exec_results.len(), 1);
    let error = exec_results[0].as_error().expect("should have error");
    assert!(
        matches!(
            error,
            EngineError::Exec(Error::Revert(ApiError::DictionaryItemKeyExceedsLength))
        ),
        "Received error {:?}",
        error
    );
}

#[ignore]
#[test]
fn dictionary_get_should_fail_with_large_item_key() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let fund_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => ACCOUNT_1_ADDR,
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    let install_contract_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        DICTIONARY_ITEM_KEY_CHECK,
        runtime_args! {
            "dictionary-operation" => "get"
        },
    )
    .build();

    builder.exec(fund_request).commit().expect_success();
    builder.exec(install_contract_request).commit();
    let exec_results = builder
        .get_exec_results()
        .last()
        .expect("should have results");
    assert_eq!(exec_results.len(), 1);
    let error = exec_results[0].as_error().expect("should have error");
    assert!(
        matches!(
            error,
            EngineError::Exec(Error::Revert(ApiError::DictionaryItemKeyExceedsLength))
        ),
        "Received error {:?}",
        error
    );
}
