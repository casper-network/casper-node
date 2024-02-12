use casper_engine_test_support::{
    utils::create_genesis_config, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder,
    ARG_AMOUNT, DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
    DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_CHAINSPEC_REGISTRY, DEFAULT_GENESIS_CONFIG_HASH,
    DEFAULT_PAYMENT, DEFAULT_PROTOCOL_VERSION, MINIMUM_ACCOUNT_CREATION_BALANCE,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error as EngineError, execution::Error};
use casper_storage::data_access_layer::GenesisRequest;
use casper_types::{
    account::AccountHash, addressable_entity::EntityKindTag, runtime_args, system::mint,
    AccessRights, AddressableEntityHash, ApiError, CLType, CLValue, GenesisAccount, Key, Motes,
    RuntimeArgs, StoredValue, U512,
};
use std::{convert::TryFrom, path::PathBuf};

use dictionary_call::{NEW_DICTIONARY_ITEM_KEY, NEW_DICTIONARY_VALUE};

const DICTIONARY_WASM: &str = "dictionary.wasm";
const DICTIONARY_CALL_WASM: &str = "dictionary_call.wasm";
const DICTIONARY_ITEM_KEY_CHECK: &str = "dictionary-item-key-check.wasm";
const DICTIONARY_READ: &str = "dictionary_read.wasm";
const READ_FROM_KEY: &str = "read_from_key.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);

fn setup() -> (LmdbWasmTestBuilder, AddressableEntityHash) {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

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

    let default_account_entity = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    assert!(default_account_entity
        .named_keys()
        .contains(dictionary::MALICIOUS_KEY_NAME));
    assert!(default_account_entity
        .named_keys()
        .contains(dictionary::DICTIONARY_REF));

    let entity_hash = default_account_entity
        .named_keys()
        .get(dictionary::CONTRACT_HASH_NAME)
        .cloned()
        .and_then(Key::into_entity_hash)
        .expect("should have hash");

    (builder, entity_hash)
}

fn query_dictionary_item(
    builder: &LmdbWasmTestBuilder,
    key: Key,
    dictionary_name: Option<String>,
    dictionary_item_key: String,
) -> Result<StoredValue, String> {
    let empty_path = vec![];
    let dictionary_key_bytes = dictionary_item_key.as_bytes();
    let address = match key {
        Key::Account(_) => {
            if dictionary_name.is_none() {
                return Err("No dictionary name was provided".to_string());
            }
            let stored_value = builder.query(None, key, &[])?;
            if let StoredValue::CLValue(cl_value) = stored_value {
                let entity_hash: AddressableEntityHash = CLValue::into_t::<Key>(cl_value)
                    .expect("must convert to contract hash")
                    .into_entity_hash()
                    .expect("must convert to contract hash");

                let entity_key = Key::addressable_entity_key(EntityKindTag::Account, entity_hash);

                return query_dictionary_item(
                    builder,
                    entity_key,
                    dictionary_name,
                    dictionary_item_key,
                );
            } else {
                return Err("Provided base key is not an account".to_string());
            }
        }
        Key::AddressableEntity(entity_addr) => {
            if let Some(name) = dictionary_name {
                let stored_value = builder.query(None, key, &[])?;

                match &stored_value {
                    StoredValue::AddressableEntity(_) => {}
                    _ => {
                        return Err(
                            "Provided base key is nether an account or a contract".to_string()
                        )
                    }
                };

                let named_keys = builder.get_named_keys(entity_addr);

                let dictionary_uref = named_keys
                    .get(&name)
                    .and_then(Key::as_uref)
                    .ok_or_else(|| "No dictionary uref was found in named keys".to_string())?;

                Key::dictionary(*dictionary_uref, dictionary_key_bytes)
            } else {
                return Err("No dictionary name was provided".to_string());
            }
        }
        Key::URef(uref) => Key::dictionary(uref, dictionary_key_bytes),
        Key::Dictionary(address) => Key::Dictionary(address),
        _ => return Err("Unsupported key type for a query to a dictionary item".to_string()),
    };
    builder.query(None, address, &empty_path)
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
        .get_entity_with_named_keys_by_entity_hash(contract_hash)
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

    let value: String = dictionary_value.into_t().expect("should be a string");
    assert_eq!(value, "Hello, world!");

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

    let value: String = dictionary_value.into_t().expect("should be a string");
    assert_eq!(value, "Hello, world! Hello, world!");
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

    let exec_results = builder.get_last_exec_result().expect("should have results");
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

    let exec_results = builder.get_last_exec_result().expect("should have results");

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

    let contract_named_keys = builder.get_named_keys_by_contract_entity_hash(contract_hash);

    let stored_dictionary_key = contract_named_keys
        .get(dictionary::DICTIONARY_NAME)
        .expect("dictionary");
    let dictionary_root_uref = stored_dictionary_key.into_uref().expect("should be uref");

    let dictionary_key = Key::dictionary(dictionary_root_uref, NEW_DICTIONARY_ITEM_KEY.as_bytes());

    let result = builder
        .query(None, dictionary_key, &[])
        .expect("should query");
    let value = result.as_cl_value().cloned().expect("should have cl value");
    let value: String = value.into_t().expect("should get string");
    assert_eq!(value, NEW_DICTIONARY_VALUE);
}

#[ignore]
#[test]
fn should_not_write_with_forged_uref() {
    let (mut builder, contract_hash) = setup();

    let contract = builder
        .get_entity_with_named_keys_by_entity_hash(contract_hash)
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

    let exec_results = builder.get_last_exec_result().expect("should have results");
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
        .get_entity_with_named_keys_by_entity_hash(contract_hash)
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
    let exec_results = builder.get_last_exec_result().expect("should have results");
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
        .get_entity_with_named_keys_by_entity_hash(contract_hash)
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
    let exec_results = builder.get_last_exec_result().expect("should have results");
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
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

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
    let exec_results = builder.get_last_exec_result().expect("should have results");
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
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

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
    let exec_results = builder.get_last_exec_result().expect("should have results");
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
fn should_query_dictionary_items_with_test_builder() {
    let genesis_account = GenesisAccount::account(
        DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        Motes::new(U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE)),
        None,
    );

    let mut accounts = vec![genesis_account];
    accounts.extend((*DEFAULT_ACCOUNTS).clone());
    let genesis_config = create_genesis_config(accounts);
    let genesis_request = GenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        genesis_config,
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    );

    let dictionary_code = PathBuf::from(DICTIONARY_WASM);
    let deploy_item = DeployItemBuilder::new()
        .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
        .with_session_code(dictionary_code, RuntimeArgs::new())
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_deploy_hash([42; 32])
        .build();

    let exec_request = ExecuteRequestBuilder::from_deploy_item(deploy_item).build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(genesis_request).commit();

    builder.exec(exec_request).commit().expect_success();

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let entity_hash = default_account
        .named_keys()
        .get(dictionary::CONTRACT_HASH_NAME)
        .expect("should have contract")
        .into_entity_hash()
        .expect("should have hash");

    let dictionary_uref = default_account
        .named_keys()
        .get(dictionary::DICTIONARY_REF)
        .expect("should have dictionary uref")
        .into_uref()
        .expect("should have URef");

    {
        // Query through account's named keys
        let queried_value = query_dictionary_item(
            &builder,
            Key::from(*DEFAULT_ACCOUNT_ADDR),
            Some(dictionary::DICTIONARY_REF.to_string()),
            dictionary::DEFAULT_DICTIONARY_NAME.to_string(),
        )
        .expect("should query");
        let value = CLValue::try_from(queried_value).expect("should have cl value");
        let value: String = value.into_t().expect("should be string");
        assert_eq!(value, dictionary::DEFAULT_DICTIONARY_VALUE);
    }

    {
        // Query through account's named keys
        let queried_value = query_dictionary_item(
            &builder,
            Key::from(*DEFAULT_ACCOUNT_ADDR),
            Some(dictionary::DICTIONARY_REF.to_string()),
            dictionary::DEFAULT_DICTIONARY_NAME.to_string(),
        )
        .expect("should query");
        let value = CLValue::try_from(queried_value).expect("should have cl value");
        let value: String = value.into_t().expect("should be string");
        assert_eq!(value, dictionary::DEFAULT_DICTIONARY_VALUE);
    }

    {
        // Query through contract's named keys
        let queried_value = query_dictionary_item(
            &builder,
            Key::addressable_entity_key(EntityKindTag::SmartContract, entity_hash),
            Some(dictionary::DICTIONARY_NAME.to_string()),
            dictionary::DEFAULT_DICTIONARY_NAME.to_string(),
        )
        .expect("should query");
        let value = CLValue::try_from(queried_value).expect("should have cl value");
        let value: String = value.into_t().expect("should be string");
        assert_eq!(value, dictionary::DEFAULT_DICTIONARY_VALUE);
    }

    {
        // Query through dictionary URef itself
        let queried_value = query_dictionary_item(
            &builder,
            Key::from(dictionary_uref),
            None,
            dictionary::DEFAULT_DICTIONARY_NAME.to_string(),
        )
        .expect("should query");
        let value = CLValue::try_from(queried_value).expect("should have cl value");
        let value: String = value.into_t().expect("should be string");
        assert_eq!(value, dictionary::DEFAULT_DICTIONARY_VALUE);
    }

    {
        // Query by computed dictionary item key
        let dictionary_item_name = dictionary::DEFAULT_DICTIONARY_NAME.as_bytes();
        let dictionary_item_key = Key::dictionary(dictionary_uref, dictionary_item_name);

        let queried_value =
            query_dictionary_item(&builder, dictionary_item_key, None, String::new())
                .expect("should query");
        let value = CLValue::try_from(queried_value).expect("should have cl value");
        let value: String = value.into_t().expect("should be string");
        assert_eq!(value, dictionary::DEFAULT_DICTIONARY_VALUE);
    }
}

#[ignore]
#[test]
fn should_be_able_to_perform_dictionary_read() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let dictionary_session_call =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, DICTIONARY_READ, RuntimeArgs::new())
            .build();

    builder
        .exec(dictionary_session_call)
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn should_be_able_to_perform_read_from_key() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let read_from_key_session_call =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, READ_FROM_KEY, RuntimeArgs::new())
            .build();

    builder
        .exec(read_from_key_session_call)
        .expect_success()
        .commit();
}
