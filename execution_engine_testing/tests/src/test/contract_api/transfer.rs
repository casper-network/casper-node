use assert_matches::assert_matches;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestContext, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_PAYMENT, DEFAULT_RUN_GENESIS_REQUEST,
    MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::core::{engine_state::Error as EngineError, execution::Error};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::{handle_payment, mint},
    ApiError, RuntimeArgs, U512,
};

const CONTRACT_TRANSFER_PURSE_TO_ACCOUNT: &str = "transfer_purse_to_account.wasm";
const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";

static TRANSFER_1_AMOUNT: Lazy<U512> =
    Lazy::new(|| U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE) + 1000);
static TRANSFER_2_AMOUNT: Lazy<U512> = Lazy::new(|| U512::from(750));
static TRANSFER_2_AMOUNT_WITH_ADV: Lazy<U512> = Lazy::new(|| *DEFAULT_PAYMENT + *TRANSFER_2_AMOUNT);
static TRANSFER_TOO_MUCH: Lazy<U512> = Lazy::new(|| U512::from(u64::max_value()));
static ACCOUNT_1_INITIAL_BALANCE: Lazy<U512> = Lazy::new(|| *DEFAULT_PAYMENT);

const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const ACCOUNT_2_ADDR: AccountHash = AccountHash::new([2u8; 32]);
const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

#[ignore]
#[test]
fn should_transfer_to_account() {
    let transfer_amount: U512 = *TRANSFER_1_AMOUNT;

    // Run genesis
    let mut builder = InMemoryWasmTestContext::default();

    let builder = builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

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
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
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
fn should_transfer_from_account_to_account() {
    let initial_genesis_amount: U512 = U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE);
    let transfer_1_amount: U512 = *TRANSFER_1_AMOUNT;
    let transfer_2_amount: U512 = *TRANSFER_2_AMOUNT;

    // Run genesis
    let mut builder = InMemoryWasmTestContext::default();

    let builder = builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

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
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
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
        .get_account(ACCOUNT_1_ADDR)
        .expect("should have account 1");
    let account_1_purse = account_1.main_purse();
    let account_1_balance = builder.get_purse_balance(account_1_purse);

    assert_eq!(account_1_balance, transfer_1_amount,);

    // Exec transfer 2 contract

    let exec_request_2 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_2_ADDR, ARG_AMOUNT => *TRANSFER_2_AMOUNT },
    )
    .build();

    let proposer_reward_starting_balance_2 = builder.get_proposer_purse_balance();

    builder.exec(exec_request_2).expect_success().commit();

    let transaction_fee_2 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_2;

    let account_2 = builder
        .get_account(ACCOUNT_2_ADDR)
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
    let mut builder = InMemoryWasmTestContext::default();

    let builder = builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

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
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();

    let proposer_reward_starting_balance_1 = builder.get_proposer_purse_balance();

    builder.exec(exec_request_1).expect_success().commit();

    // Exec transfer contract

    let account_1 = builder
        .get_account(ACCOUNT_1_ADDR)
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
        ACCOUNT_1_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_2_ADDR, ARG_AMOUNT => *TRANSFER_2_AMOUNT },
    )
    .build();

    let proposer_reward_starting_balance_2 = builder.get_proposer_purse_balance();

    builder.exec(exec_request_2).expect_success().commit();

    let account_2 = builder
        .get_account(ACCOUNT_2_ADDR)
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
        runtime_args! { ARG_TARGET => ACCOUNT_1_ADDR, ARG_AMOUNT => *TRANSFER_1_AMOUNT },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_2_ADDR, ARG_AMOUNT => *TRANSFER_2_AMOUNT_WITH_ADV },
    )
    .build();

    let exec_request_3 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => ACCOUNT_2_ADDR, ARG_AMOUNT => *TRANSFER_TOO_MUCH },
    )
    .build();

    let result = InMemoryWasmTestContext::default()
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
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
        .commit()
        .finish();

    let exec_results = result
        .builder()
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
    let mut builder = InMemoryWasmTestContext::default();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { "target" => ACCOUNT_1_ADDR, "amount" => *ACCOUNT_1_INITIAL_BALANCE },
    )
    .build();

    let exec_request_2 = ExecuteRequestBuilder::standard(
        ACCOUNT_1_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { "target" => ACCOUNT_2_ADDR, "amount" => *ACCOUNT_1_INITIAL_BALANCE },
    )
    .build();
    builder
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .exec(exec_request_2)
        .commit()
        .expect_success()
        .finish();
}
