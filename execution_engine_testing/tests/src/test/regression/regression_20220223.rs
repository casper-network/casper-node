use casper_types::system::mint;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_PUBLIC_KEY,
    MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    engine_state, engine_state::engine_config::DEFAULT_MINIMUM_DELEGATION_AMOUNT, execution,
};
use casper_types::{
    self,
    account::AccountHash,
    api_error::ApiError,
    runtime_args,
    system::auction::{
        DelegationRate, ARG_AMOUNT, ARG_DELEGATION_RATE, ARG_DELEGATOR, ARG_PUBLIC_KEY,
        ARG_VALIDATOR,
    },
    PublicKey, SecretKey, U512,
};

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_TRANSFER_TO_NAMED_PURSE: &str = "transfer_to_named_purse.wasm";

const CONTRACT_REGRESSION_ADD_BID: &str = "regression_add_bid.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";

const CONTRACT_REGRESSION_DELEGATE: &str = "regression_delegate.wasm";
const CONTRACT_DELEGATE: &str = "delegate.wasm";

const CONTRACT_REGRESSION_TRANSFER: &str = "regression_transfer.wasm";

const ARG_TARGET: &str = "target";
const ARG_PURSE_NAME: &str = "purse_name";
const TEST_PURSE: &str = "test_purse";
const TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE + 1000;
const ADD_BID_AMOUNT_1: u64 = 95_000;
const ADD_BID_DELEGATION_RATE_1: DelegationRate = 10;
const DELEGATE_AMOUNT_1: u64 = 125_000 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;

static VALIDATOR_1_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([3; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALIDATOR_1_ADDR: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*VALIDATOR_1_PUBLIC_KEY));

#[ignore]
#[test]
fn should_fail_to_add_new_bid_over_the_approved_amount() {
    let mut builder = setup();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_REGRESSION_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1_PUBLIC_KEY.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(validator_1_add_bid_request).expect_failure();

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::UnapprovedSpendingAmount as u8
        ),
        "Expected unapproved spending amount error but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_fail_to_add_into_existing_bid_over_the_approved_amount() {
    let mut builder = setup();

    let validator_1_add_bid_request_1 = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1_PUBLIC_KEY.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let validator_1_add_bid_request_2 = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_REGRESSION_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1_PUBLIC_KEY.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder
        .exec(validator_1_add_bid_request_1)
        .expect_success()
        .commit();
    builder.exec(validator_1_add_bid_request_2).expect_failure();

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::UnapprovedSpendingAmount as u8
        ),
        "Expected unapproved spending amount error but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_fail_to_add_new_delegator_over_the_approved_amount() {
    let mut builder = setup();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1_PUBLIC_KEY.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder
        .exec(validator_1_add_bid_request)
        .expect_success()
        .commit();

    let delegator_1_delegate_requestr = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_REGRESSION_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => VALIDATOR_1_PUBLIC_KEY.clone(),
            ARG_DELEGATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        },
    )
    .build();

    builder
        .exec(delegator_1_delegate_requestr)
        .expect_failure()
        .commit();

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::UnapprovedSpendingAmount as u8
        ),
        "Expected unapproved spending amount error but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_fail_to_update_existing_delegator_over_the_approved_amount() {
    let mut builder = setup();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1_PUBLIC_KEY.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let delegator_1_delegate_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => VALIDATOR_1_PUBLIC_KEY.clone(),
            ARG_DELEGATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        },
    )
    .build();

    let delegator_1_delegate_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_REGRESSION_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => VALIDATOR_1_PUBLIC_KEY.clone(),
            ARG_DELEGATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        },
    )
    .build();

    builder
        .exec(validator_1_add_bid_request)
        .expect_success()
        .commit();

    builder
        .exec(delegator_1_delegate_request_1)
        .expect_success()
        .commit();

    builder
        .exec(delegator_1_delegate_request_2)
        .expect_failure()
        .commit();

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::UnapprovedSpendingAmount as u8
        ),
        "Expected unapproved spending amount error but received {:?}",
        error
    );
}

#[ignore]
#[test]
fn should_fail_to_mint_transfer_over_the_limit() {
    let mut builder = setup();

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let test_purse_2 = default_account
        .named_keys()
        .get(TEST_PURSE)
        .unwrap()
        .into_uref()
        .expect("should have test purse 2");

    let args = runtime_args! {
        mint::ARG_TO => Option::<AccountHash>::None,
        mint::ARG_TARGET => test_purse_2,
        mint::ARG_AMOUNT => U512::one(),
        mint::ARG_ID => Some(1u64),
    };
    let transfer_request_1 =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, CONTRACT_REGRESSION_TRANSFER, args)
            .build();

    builder.exec(transfer_request_1).expect_failure().commit();

    let error = builder.get_error().expect("should have returned an error");
    assert!(
        matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::Mint(mint_error)))
            if mint_error == mint::Error::UnapprovedSpendingAmount as u8
        ),
        "Expected unapproved spending amount error but received {:?}",
        error
    );
}

fn setup() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());
    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let create_purse_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_NAMED_PURSE,
        runtime_args! {
            ARG_PURSE_NAME => TEST_PURSE,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
        },
    )
    .build();

    builder
        .exec(validator_1_fund_request)
        .expect_success()
        .commit();

    builder.exec(create_purse_request).expect_success().commit();

    builder
}
