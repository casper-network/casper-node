use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution::Error as ExecError};
use casper_types::{account::AccountHash, runtime_args, system::mint, ApiError, U512};

const REGRESSION_20220207_CONTRACT: &str = "regression_20220207.wasm";
const ARG_AMOUNT_TO_SEND: &str = "amount_to_send";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([111; 32]);

const UNAPPROVED_SPENDING_AMOUNT_ERR: Error = Error::Exec(ExecError::Revert(ApiError::Mint(
    mint::Error::UnapprovedSpendingAmount as u8,
)));

#[ignore]
#[test]
fn should_not_transfer_above_approved_limit() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let args = runtime_args! {
        mint::ARG_AMOUNT => U512::from(1000u64), // What we approved.
        ARG_AMOUNT_TO_SEND => U512::from(1100u64), // What contract is trying to send.
        mint::ARG_TARGET => ACCOUNT_1_ADDR,
    };

    let exec_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, REGRESSION_20220207_CONTRACT, args)
            .build();

    builder
        .exec(exec_request)
        .assert_error(UNAPPROVED_SPENDING_AMOUNT_ERR);
}

#[ignore]
#[test]
fn should_transfer_within_approved_limit() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let args = runtime_args! {
        mint::ARG_AMOUNT => U512::from(1000u64),
        ARG_AMOUNT_TO_SEND => U512::from(100u64),
        mint::ARG_TARGET => ACCOUNT_1_ADDR,
    };

    let exec_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, REGRESSION_20220207_CONTRACT, args)
            .build();

    builder.exec(exec_request).expect_success();
}

#[ignore]
#[test]
fn should_fail_without_amount_arg() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let args = runtime_args! {
        // If `amount` arg is absent, host assumes that limit is 0.
        // This should fail then.
        ARG_AMOUNT_TO_SEND => U512::from(100u64),
        mint::ARG_TARGET => ACCOUNT_1_ADDR,
    };

    let exec_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, REGRESSION_20220207_CONTRACT, args)
            .build();

    builder
        .exec(exec_request)
        .assert_error(UNAPPROVED_SPENDING_AMOUNT_ERR);
}
