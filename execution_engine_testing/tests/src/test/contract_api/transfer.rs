use assert_matches::assert_matches;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_PAYMENT, MINIMUM_ACCOUNT_CREATION_BALANCE,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::{engine_state::Error as EngineError, execution::Error};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::{handle_payment, mint},
    ApiError, PublicKey, RuntimeArgs, SecretKey, U512,
};

const CONTRACT_TRANSFER_PURSE_TO_ACCOUNT: &str = "transfer_purse_to_account.wasm";
const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_TRANSFER_TO_PUBLIC_KEY: &str = "transfer_to_public_key.wasm";
const CONTRACT_TRANSFER_PURSE_TO_PUBLIC_KEY: &str = "transfer_purse_to_public_key.wasm";
const CONTRACT_TRANSFER_TO_NAMED_PURSE: &str = "transfer_to_named_purse.wasm";

static TRANSFER_1_AMOUNT: Lazy<U512> =
    Lazy::new(|| U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE) + 1000);
static TRANSFER_2_AMOUNT: Lazy<U512> = Lazy::new(|| U512::from(750));
static TRANSFER_2_AMOUNT_WITH_ADV: Lazy<U512> = Lazy::new(|| *DEFAULT_PAYMENT + *TRANSFER_2_AMOUNT);
static TRANSFER_TOO_MUCH: Lazy<U512> = Lazy::new(|| U512::from(u64::max_value()));
static ACCOUNT_1_INITIAL_BALANCE: Lazy<U512> =
    Lazy::new(|| U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE));

static ACCOUNT_1_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes(&[234u8; 32]).unwrap());
static ACCOUNT_1_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_1_SECRET_KEY));
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_1_PUBLIC_KEY.to_account_hash());

static ACCOUNT_2_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes(&[210u8; 32]).unwrap());
static ACCOUNT_2_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_2_SECRET_KEY));
static ACCOUNT_2_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_2_PUBLIC_KEY.to_account_hash());

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";
const ARG_SOURCE_PURSE: &str = "source_purse";
const ARG_PURSE_NAME: &str = "purse_name";
const TEST_PURSE: &str = "test_purse";

#[ignore]
#[test]
fn should_transfer_to_account() {
    let transfer_amount: U512 = *TRANSFER_1_AMOUNT;

    // Run genesis
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");

    let default_account_purse = default_account.main_purse();

    // Check genesis account balance
    let initial_account_balance = builder.get_purse_balance(default_account_purse);

    // Exec transfer contract

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => *ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(exec_request_1).expect_success().commit();

    // Check genesis account balance

    let modified_balance = builder.get_purse_balance(default_account_purse);

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    assert_eq!(
        modified_balance,
        initial_account_balance - transaction_fee - transfer_amount
    );

    let handle_payment = builder.get_handle_payment_contract();
    let payment_purse = (*handle_payment
        .named_keys()
        .get(handle_payment::PAYMENT_PURSE_KEY)
        .unwrap())
    .into_uref()
    .unwrap();
    assert_eq!(builder.get_purse_balance(payment_purse), U512::zero());
}

#[ignore]
#[test]
fn should_transfer_to_public_key() {
    let transfer_amount: U512 = *TRANSFER_1_AMOUNT;

    // Run genesis
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");

    let default_account_purse = default_account.main_purse();

    // Check genesis account balance
    let initial_account_balance = builder.get_purse_balance(default_account_purse);

    // Exec transfer contract

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_PUBLIC_KEY,
        runtime_args! { ARG_TARGET => ACCOUNT_1_PUBLIC_KEY.clone(), ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(exec_request_1).expect_success().commit();

    // Check genesis account balance

    let modified_balance = builder.get_purse_balance(default_account_purse);

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    assert_eq!(
        modified_balance,
        initial_account_balance - transaction_fee - transfer_amount
    );

    let handle_payment = builder.get_handle_payment_contract();
    let payment_purse = (*handle_payment
        .named_keys()
        .get(handle_payment::PAYMENT_PURSE_KEY)
        .unwrap())
    .into_uref()
    .unwrap();
    assert_eq!(builder.get_purse_balance(payment_purse), U512::zero());
}

#[ignore]
#[test]
fn should_transfer_from_purse_to_public_key() {
    // Run genesis
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    // Create a funded a purse, and store it in named keys
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_NAMED_PURSE,
        runtime_args! {
            ARG_PURSE_NAME => TEST_PURSE,
            ARG_AMOUNT => *TRANSFER_1_AMOUNT,
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");
    let default_account_purse = default_account.main_purse();

    // Check genesis account balance
    let initial_account_balance = builder.get_purse_balance(default_account_purse);

    let test_purse = default_account.named_keys()[TEST_PURSE]
        .into_uref()
        .expect("should have test purse");

    let test_purse_balanace_before = builder.get_purse_balance(test_purse);

    // Exec transfer contract
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_PUBLIC_KEY,
        runtime_args! {
            ARG_SOURCE_PURSE => test_purse,
            ARG_TARGET => ACCOUNT_1_PUBLIC_KEY.clone(),
            ARG_AMOUNT => *TRANSFER_1_AMOUNT,
        },
    )
    .build();

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(exec_request_2).expect_success().commit();

    // Check genesis account balance

    let modified_balance = builder.get_purse_balance(default_account_purse);

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    assert_eq!(modified_balance, initial_account_balance - transaction_fee);

    let test_purse_balanace_after = builder.get_purse_balance(test_purse);
    assert_eq!(
        test_purse_balanace_after,
        test_purse_balanace_before - *TRANSFER_1_AMOUNT
    );

    let handle_payment = builder.get_handle_payment_contract();
    let payment_purse = (*handle_payment
        .named_keys()
        .get(handle_payment::PAYMENT_PURSE_KEY)
        .unwrap())
    .into_uref()
    .unwrap();
    assert_eq!(builder.get_purse_balance(payment_purse), U512::zero());
}

#[ignore]
#[test]
fn should_transfer_from_account_to_account() {
    let initial_genesis_amount: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);
    let transfer_1_amount: U512 = *TRANSFER_1_AMOUNT;
    let transfer_2_amount: U512 = *TRANSFER_2_AMOUNT;

    // Run genesis
    let mut builder = InMemoryWasmTestBuilder::default();

    let builder = builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");

    let default_account_purse = default_account.main_purse();

    // Check genesis account balance
    let genesis_balance = builder.get_purse_balance(default_account_purse);

    assert_eq!(genesis_balance, initial_genesis_amount,);

    // Exec transfer 1 contract

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => *ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let proposer_reward_starting_balance_1 = builder.get_proposer_purse_balance();

    builder.exec(exec_request_1).expect_success().commit();

    let modified_balance = builder.get_purse_balance(default_account_purse);

    let transaction_fee_1 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_1;

    let expected_balance = initial_genesis_amount - transaction_fee_1 - transfer_1_amount;

    assert_eq!(modified_balance, expected_balance);

    // Check account 1 balance
    let account_1 = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account 1");
    let account_1_purse = account_1.main_purse();
    let account_1_balance = builder.get_purse_balance(account_1_purse);

    assert_eq!(account_1_balance, transfer_1_amount,);

    // Exec transfer 2 contract

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => *ACCOUNT_2_ADDR, ARG_AMOUNT => *TRANSFER_2_AMOUNT },
    )
    .build();

    let proposer_reward_starting_balance_2 = builder.get_proposer_purse_balance();

    builder.exec(exec_request_2).expect_success().commit();

    let transaction_fee_2 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_2;

    let account_2 = builder
        .get_account(*ACCOUNT_2_ADDR)
        .expect("should have account 2");

    let account_2_purse = account_2.main_purse();

    // Check account 1 balance

    let account_1_balance = builder.get_purse_balance(account_1_purse);

    assert_eq!(
        account_1_balance,
        transfer_1_amount - transaction_fee_2 - transfer_2_amount
    );

    let account_2_balance = builder.get_purse_balance(account_2_purse);

    assert_eq!(account_2_balance, transfer_2_amount,);
}

#[ignore]
#[test]
fn should_transfer_to_existing_account() {
    let initial_genesis_amount: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);
    let transfer_1_amount: U512 = *TRANSFER_1_AMOUNT;
    let transfer_2_amount: U512 = *TRANSFER_2_AMOUNT;

    // Run genesis
    let mut builder = InMemoryWasmTestBuilder::default();

    let builder = builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");

    let default_account_purse = default_account.main_purse();

    // Check genesis account balance
    let genesis_balance = builder.get_purse_balance(default_account_purse);

    assert_eq!(genesis_balance, initial_genesis_amount,);

    // Exec transfer 1 contract

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => *ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let proposer_reward_starting_balance_1 = builder.get_proposer_purse_balance();

    builder.exec(exec_request_1).expect_success().commit();

    // Exec transfer contract

    let account_1 = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should get account");

    let account_1_purse = account_1.main_purse();

    // Check genesis account balance

    let genesis_balance = builder.get_purse_balance(default_account_purse);

    let transaction_fee_1 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_1;

    assert_eq!(
        genesis_balance,
        initial_genesis_amount - transaction_fee_1 - transfer_1_amount
    );

    // Check account 1 balance

    let account_1_balance = builder.get_purse_balance(account_1_purse);

    assert_eq!(account_1_balance, transfer_1_amount,);

    // Exec transfer contract

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => *ACCOUNT_2_ADDR, ARG_AMOUNT => *TRANSFER_2_AMOUNT },
    )
    .build();

    let proposer_reward_starting_balance_2 = builder.get_proposer_purse_balance();

    builder.exec(exec_request_2).expect_success().commit();

    let account_2 = builder
        .get_account(*ACCOUNT_2_ADDR)
        .expect("should get account");

    let account_2_purse = account_2.main_purse();

    // Check account 1 balance

    let account_1_balance = builder.get_purse_balance(account_1_purse);

    let transaction_fee_2 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_2;

    assert_eq!(
        account_1_balance,
        transfer_1_amount - transaction_fee_2 - transfer_2_amount,
    );

    // Check account 2 balance

    let account_2_balance_transform = builder.get_purse_balance(account_2_purse);

    assert_eq!(account_2_balance_transform, transfer_2_amount);
}

#[ignore]
#[test]
fn should_fail_when_insufficient_funds() {
    // Run genesis

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => *ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => *ACCOUNT_2_ADDR, ARG_AMOUNT => *TRANSFER_2_AMOUNT_WITH_ADV },
    )
    .build();

    let exec_request_3 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => *ACCOUNT_2_ADDR, ARG_AMOUNT => *TRANSFER_TOO_MUCH },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();
    builder
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        // Exec transfer contract
        .exec(exec_request_1)
        .expect_success()
        .commit()
        // Exec transfer contract
        .exec(exec_request_2)
        .expect_success()
        .commit()
        // Exec transfer contract
        .exec(exec_request_3)
        .commit();

    let exec_results = builder
        .get_exec_result(2)
        .expect("should have exec response");
    assert_eq!(exec_results.len(), 1);
    let exec_result = exec_results[0].as_error().expect("should have error");
    let error = assert_matches!(exec_result, EngineError::Exec(Error::Revert(e)) => *e, "{:?}", exec_result);
    assert_eq!(error, ApiError::from(mint::Error::InsufficientFunds));
}

#[ignore]
#[test]
fn should_transfer_total_amount() {
    let mut builder = InMemoryWasmTestBuilder::default();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { "target" => *ACCOUNT_1_ADDR, "amount" => *ACCOUNT_1_INITIAL_BALANCE },
    )
    .build();

    let transfer_amount_1 = *ACCOUNT_1_INITIAL_BALANCE - *DEFAULT_PAYMENT;

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { "target" => *ACCOUNT_2_ADDR, "amount" => transfer_amount_1 },
    )
    .build();

    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    builder.exec(exec_request_1).expect_success().commit();

    builder.exec(exec_request_2).commit().expect_success();

    let account_1 = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("should have account");
    let account_1_main_purse = account_1.main_purse();
    let account_1_balance = builder.get_purse_balance(account_1_main_purse);

    assert_eq!(account_1_balance, U512::zero());
}
