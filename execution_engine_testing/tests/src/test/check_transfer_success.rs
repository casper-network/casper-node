use core::convert::TryFrom;
use std::path::PathBuf;

use casper_engine_test_support::{
    utils::create_genesis_config, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder,
    DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
    DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_CHAINSPEC_REGISTRY, DEFAULT_GENESIS_CONFIG_HASH,
    DEFAULT_PAYMENT, DEFAULT_PROTOCOL_VERSION,
};
use casper_storage::data_access_layer::GenesisRequest;
use casper_types::{runtime_args, GenesisAccount, Key, Motes, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_DESTINATION: &str = "destination";
const TRANSFER_WASM: &str = "transfer_main_purse_to_new_purse.wasm";
const NEW_PURSE_NAME: &str = "test_purse";
const FIRST_TRANSFER_AMOUNT: u64 = 142;
const SECOND_TRANSFER_AMOUNT: u64 = 250;

#[ignore]
#[test]
fn test_check_transfer_success_with_source_only() {
    // create a genesis account.
    let genesis_account = GenesisAccount::account(
        DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        Motes::new(U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE)),
        None,
    );

    // add the account to the genesis config.
    let mut accounts = vec![genesis_account];
    accounts.extend((*DEFAULT_ACCOUNTS).clone());
    let genesis_config = create_genesis_config(accounts);
    let genesis_request = GenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        genesis_config,
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    );

    // Doing a transfer from main purse to create new purse and store URef under NEW_PURSE_NAME.
    let transfer_amount = U512::try_from(FIRST_TRANSFER_AMOUNT).expect("U512 from u64");
    let path = PathBuf::from(TRANSFER_WASM);
    let session_args = runtime_args! {
        ARG_DESTINATION => NEW_PURSE_NAME,
        ARG_AMOUNT => transfer_amount
    };

    // build the deploy.
    let deploy_item = DeployItemBuilder::new()
        .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
        .with_session_code(path, session_args)
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_deploy_hash([42; 32])
        .build();

    // build a request to execute the deploy.
    let exec_request = ExecuteRequestBuilder::from_deploy_item(deploy_item).build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(genesis_request).commit();

    // we need this to figure out what the transfer fee is.
    let proposer_starting_balance = builder.get_proposer_purse_balance();

    // Getting main purse URef to verify transfer
    let source_purse = builder
        .get_expected_addressable_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .main_purse();

    builder.exec(exec_request).commit().expect_success();

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_starting_balance;
    let expected_source_ending_balance = Motes::new(U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE))
        - Motes::new(transfer_amount)
        - Motes::new(transaction_fee);
    let actual_source_ending_balance = Motes::new(builder.get_purse_balance(source_purse));

    assert_eq!(expected_source_ending_balance, actual_source_ending_balance);
}

#[ignore]
#[test]
fn test_check_transfer_success_with_source_only_errors() {
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

    // Doing a transfer from main purse to create new purse and store Uref under NEW_PURSE_NAME.
    let transfer_amount = U512::try_from(FIRST_TRANSFER_AMOUNT).expect("U512 from u64");
    // Setup mismatch between transfer_amount performed and given to trigger assertion.
    let wrong_transfer_amount = transfer_amount - U512::try_from(100u64).expect("U512 from 64");

    let path = PathBuf::from(TRANSFER_WASM);
    let session_args = runtime_args! {
        ARG_DESTINATION => NEW_PURSE_NAME,
        ARG_AMOUNT => wrong_transfer_amount
    };

    let deploy_item = DeployItemBuilder::new()
        .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
        .with_session_code(path, session_args)
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_deploy_hash([42; 32])
        .build();

    let exec_request = ExecuteRequestBuilder::from_deploy_item(deploy_item).build();

    // Set up test builder and run genesis.
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(genesis_request).commit();

    // compare proposer balance before and after the transaction to get the tx fee.
    let proposer_starting_balance = builder.get_proposer_purse_balance();
    let source_purse = builder
        .get_expected_addressable_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .main_purse();

    builder.exec(exec_request).commit().expect_success();

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_starting_balance;
    let expected_source_ending_balance = Motes::new(U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE))
        - Motes::new(transfer_amount)
        - Motes::new(transaction_fee);
    let actual_source_ending_balance = Motes::new(builder.get_purse_balance(source_purse));

    assert!(expected_source_ending_balance != actual_source_ending_balance);
}

#[ignore]
#[test]
fn test_check_transfer_success_with_source_and_target() {
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

    let transfer_amount = U512::try_from(SECOND_TRANSFER_AMOUNT).expect("U512 from u64");
    // Doing a transfer from main purse to create new purse and store URef under NEW_PURSE_NAME.
    let path = PathBuf::from(TRANSFER_WASM);
    let session_args = runtime_args! {
        ARG_DESTINATION => NEW_PURSE_NAME,
        ARG_AMOUNT => transfer_amount
    };
    let deploy_item = DeployItemBuilder::new()
        .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => *DEFAULT_PAYMENT})
        .with_session_code(path, session_args)
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_deploy_hash([42; 32])
        .build();

    let exec_request = ExecuteRequestBuilder::from_deploy_item(deploy_item).build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(genesis_request).commit();

    // we need this to figure out what the transfer fee is.
    let proposer_starting_balance = builder.get_proposer_purse_balance();

    // Getting main purse URef to verify transfer
    let source_purse = builder
        .get_expected_addressable_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .main_purse();

    builder.exec(exec_request).commit().expect_success();

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_starting_balance;
    let expected_source_ending_balance = Motes::new(U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE))
        - Motes::new(transfer_amount)
        - Motes::new(transaction_fee);
    let actual_source_ending_balance = Motes::new(builder.get_purse_balance(source_purse));

    assert_eq!(expected_source_ending_balance, actual_source_ending_balance);

    // retrieve newly created purse URef
    builder
        .query(
            None,
            Key::Account(*DEFAULT_ACCOUNT_ADDR),
            &[NEW_PURSE_NAME.to_string()],
        )
        .expect("new purse should exist");

    // let target_purse = builder

    let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

    let target_purse = named_keys
        .get(NEW_PURSE_NAME)
        .expect("value")
        .into_uref()
        .expect("uref");

    let expected_balance = U512::from(SECOND_TRANSFER_AMOUNT);
    let target_balance = builder.get_purse_balance(target_purse);

    assert_eq!(expected_balance, target_balance);
}
