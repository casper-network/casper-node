use casper_execution_engine::{engine_state, execution};
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::{
        auction::ARG_AMOUNT,
        mint::{self, ARG_TARGET},
    },
    ApiError, U512,
};

const CONTRACT_TRANSFER_PURSE_TO_ACCOUNT: &str = "transfer_purse_to_account.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([42u8; 32]);

static ACCOUNT_1_INITIAL_FUND: Lazy<U512> = Lazy::new(|| *DEFAULT_PAYMENT + 42);

#[ignore]
#[test]
fn should_run_purse_to_account_transfer() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let account_1_account_hash = ACCOUNT_1_ADDR;
    assert!(
        builder
            .get_entity_by_account_hash(account_1_account_hash)
            .is_none(),
        "new account shouldn't exist yet"
    );

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => account_1_account_hash, ARG_AMOUNT => *ACCOUNT_1_INITIAL_FUND },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    let new_account = builder
        .get_entity_by_account_hash(account_1_account_hash)
        .expect("new account should exist now");

    let balance = builder.get_purse_balance(new_account.main_purse());

    assert_eq!(
        balance, *ACCOUNT_1_INITIAL_FUND,
        "balance should equal transferred amount"
    );
}

#[ignore]
#[test]
fn should_fail_when_sending_too_much_from_purse_to_account() {
    let account_1_key = ACCOUNT_1_ADDR;

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { "target" => account_1_key, "amount" => U512::max_value() },
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    builder.exec(exec_request_1).expect_failure().commit();

    // Get transforms output for genesis account
    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get genesis account");

    let final_balance = builder.get_purse_balance(default_account.main_purse());

    // When trying to send too much coins the balance is left unchanged
    assert_eq!(
        final_balance,
        U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE) - *DEFAULT_PAYMENT
            + builder.calculate_refund_amount(*DEFAULT_PAYMENT),
        "final balance incorrect"
    );

    let error = builder.get_error().expect("should have error");

    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::InsufficientFunds as u8,
        ),
        "Error received {:?}",
        error,
    );
}
