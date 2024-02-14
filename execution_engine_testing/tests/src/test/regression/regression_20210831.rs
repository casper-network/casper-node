use once_cell::sync::Lazy;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_PUBLIC_KEY,
    MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    engine_state::{engine_config::DEFAULT_MINIMUM_DELEGATION_AMOUNT, Error as CoreError},
    execution::Error as ExecError,
};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::{
        auction::{self, BidsExt, DelegationRate},
        mint,
    },
    ApiError, PublicKey, RuntimeArgs, SecretKey, U512,
};

static ACCOUNT_1_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([57; 32]).unwrap());
static ACCOUNT_1_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_1_SECRET_KEY));
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_1_PUBLIC_KEY));

static ACCOUNT_2_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([75; 32]).unwrap());
static ACCOUNT_2_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_2_SECRET_KEY));
static ACCOUNT_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_2_PUBLIC_KEY));

const CONTRACT_REGRESSION_20210831: &str = "regression_20210831.wasm";

const METHOD_ADD_BID_PROXY_CALL: &str = "add_bid_proxy_call";
const METHOD_WITHDRAW_PROXY_CALL: &str = "withdraw_proxy_call";
const METHOD_DELEGATE_PROXY_CALL: &str = "delegate_proxy_call";
const METHOD_UNDELEGATE_PROXY_CALL: &str = "undelegate_proxy_call";
const METHOD_ACTIVATE_BID_CALL: &str = "activate_bid_proxy_call";

const CONTRACT_HASH_NAME: &str = "contract_hash";

const BID_DELEGATION_RATE: DelegationRate = 42;
static BID_AMOUNT: Lazy<U512> = Lazy::new(|| U512::from(1_000_000));
static DELEGATE_AMOUNT: Lazy<U512> = Lazy::new(|| U512::from(500_000));

fn setup() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let id: Option<u64> = None;

    let transfer_args_1 = runtime_args! {
        mint::ARG_TARGET => *ACCOUNT_1_ADDR,
        mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
        mint::ARG_ID => id,
    };

    let transfer_request_1 =
        ExecuteRequestBuilder::transfer(*DEFAULT_ACCOUNT_ADDR, transfer_args_1).build();

    builder.exec(transfer_request_1).expect_success().commit();

    let transfer_args_2 = runtime_args! {
        mint::ARG_TARGET => *ACCOUNT_2_ADDR,
        mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
        mint::ARG_ID => id,
    };

    let transfer_request_2 =
        ExecuteRequestBuilder::transfer(*DEFAULT_ACCOUNT_ADDR, transfer_args_2).build();

    builder.exec(transfer_request_2).expect_success().commit();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let id: Option<u64> = None;

    let transfer_args_1 = runtime_args! {
        mint::ARG_TARGET => *ACCOUNT_1_ADDR,
        mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
        mint::ARG_ID => id,
    };

    let transfer_request_1 =
        ExecuteRequestBuilder::transfer(*DEFAULT_ACCOUNT_ADDR, transfer_args_1).build();

    builder.exec(transfer_request_1).expect_success().commit();

    let transfer_args_2 = runtime_args! {
        mint::ARG_TARGET => *ACCOUNT_2_ADDR,
        mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
        mint::ARG_ID => id,
    };

    let transfer_request_2 =
        ExecuteRequestBuilder::transfer(*DEFAULT_ACCOUNT_ADDR, transfer_args_2).build();

    builder.exec(transfer_request_2).expect_success().commit();

    let install_request_1 = ExecuteRequestBuilder::standard(
        *ACCOUNT_2_ADDR,
        CONTRACT_REGRESSION_20210831,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_request_1).expect_success().commit();

    builder
}

#[ignore]
#[test]
fn regression_20210831_should_fail_to_add_bid() {
    let mut builder = setup();

    let sender = *ACCOUNT_2_ADDR;
    let add_bid_args = runtime_args! {
        auction::ARG_PUBLIC_KEY => ACCOUNT_1_PUBLIC_KEY.clone(),
        auction::ARG_AMOUNT => *BID_AMOUNT,
        auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
    };

    let add_bid_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        sender,
        builder.get_auction_contract_hash(),
        auction::METHOD_ADD_BID,
        add_bid_args.clone(),
    )
    .build();

    builder.exec(add_bid_request_1);

    let error_1 = builder
        .get_error()
        .expect("attempt 1 should raise invalid context");
    assert!(
        matches!(error_1, CoreError::Exec(ExecError::Revert(ApiError::AuctionError(error_code))) if error_code == auction::Error::InvalidContext as u8),
        "{:?}",
        error_1
    );

    // ACCOUNT_2 unbonds ACCOUNT_1 through a proxy
    let add_bid_request_2 = ExecuteRequestBuilder::contract_call_by_name(
        sender,
        CONTRACT_HASH_NAME,
        METHOD_ADD_BID_PROXY_CALL,
        add_bid_args,
    )
    .build();

    builder.exec(add_bid_request_2).commit();

    let error_2 = builder
        .get_error()
        .expect("attempt 2 should raise invalid context");
    assert!(
        matches!(error_2, CoreError::Exec(ExecError::Revert(ApiError::AuctionError(error_code))) if error_code == auction::Error::InvalidContext as u8),
        "{:?}",
        error_2
    );
}

#[ignore]
#[test]
fn regression_20210831_should_fail_to_delegate() {
    let mut builder = setup();

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        builder.get_auction_contract_hash(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => *BID_AMOUNT,
            auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
        },
    )
    .build();

    builder.exec(add_bid_request).expect_success().commit();

    let sender = *ACCOUNT_2_ADDR;
    let delegate_args = runtime_args! {
        auction::ARG_VALIDATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        auction::ARG_DELEGATOR => ACCOUNT_1_PUBLIC_KEY.clone(),
        auction::ARG_AMOUNT => *DELEGATE_AMOUNT,
    };

    let delegate_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        sender,
        builder.get_auction_contract_hash(),
        auction::METHOD_DELEGATE,
        delegate_args.clone(),
    )
    .build();

    builder.exec(delegate_request_1);

    let error_1 = builder
        .get_error()
        .expect("attempt 1 should raise invalid context");
    assert!(
        matches!(error_1, CoreError::Exec(ExecError::Revert(ApiError::AuctionError(error_code))) if error_code == auction::Error::InvalidContext as u8),
        "{:?}",
        error_1
    );

    // ACCOUNT_2 unbonds ACCOUNT_1 through a proxy
    let delegate_request_2 = ExecuteRequestBuilder::contract_call_by_name(
        sender,
        CONTRACT_HASH_NAME,
        METHOD_DELEGATE_PROXY_CALL,
        delegate_args,
    )
    .build();

    builder.exec(delegate_request_2).commit();

    let error_2 = builder
        .get_error()
        .expect("attempt 2 should raise invalid context");
    assert!(
        matches!(error_2, CoreError::Exec(ExecError::Revert(ApiError::AuctionError(error_code))) if error_code == auction::Error::InvalidContext as u8),
        "{:?}",
        error_2
    );
}

#[ignore]
#[test]
fn regression_20210831_should_fail_to_withdraw_bid() {
    let mut builder = setup();

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *ACCOUNT_1_ADDR,
        builder.get_auction_contract_hash(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => ACCOUNT_1_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => *BID_AMOUNT,
            auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
        },
    )
    .build();

    builder.exec(add_bid_request).expect_success().commit();

    let bids = builder.get_bids();
    let account_1_bid_before = bids
        .validator_bid(&ACCOUNT_1_PUBLIC_KEY)
        .expect("validator bid should exist");
    assert_eq!(
        builder.get_purse_balance(*account_1_bid_before.bonding_purse()),
        *BID_AMOUNT,
    );
    assert!(
        !account_1_bid_before.inactive(),
        "newly added bid should be active"
    );

    let sender = *ACCOUNT_2_ADDR;
    let withdraw_bid_args = runtime_args! {
        auction::ARG_PUBLIC_KEY => ACCOUNT_1_PUBLIC_KEY.clone(),
        auction::ARG_AMOUNT => *BID_AMOUNT,
    };

    // ACCOUNT_2 unbonds ACCOUNT_1 by a direct auction contract call
    let exec_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        sender,
        builder.get_auction_contract_hash(),
        auction::METHOD_WITHDRAW_BID,
        withdraw_bid_args.clone(),
    )
    .build();

    builder.exec(exec_request_1).commit();

    let error_1 = builder
        .get_error()
        .expect("attempt 1 should raise invalid context");
    assert!(
        matches!(error_1, CoreError::Exec(ExecError::Revert(ApiError::AuctionError(error_code))) if error_code == auction::Error::InvalidContext as u8),
        "{:?}",
        error_1
    );

    // ACCOUNT_2 unbonds ACCOUNT_1 through a proxy
    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_name(
        sender,
        CONTRACT_HASH_NAME,
        METHOD_WITHDRAW_PROXY_CALL,
        withdraw_bid_args,
    )
    .build();

    builder.exec(exec_request_2).commit();

    let error_2 = builder
        .get_error()
        .expect("attempt 2 should raise invalid context");
    assert!(
        matches!(error_2, CoreError::Exec(ExecError::Revert(ApiError::AuctionError(error_code))) if error_code == auction::Error::InvalidContext as u8),
        "{:?}",
        error_2
    );

    let bids = builder.get_bids();
    let account_1_bid_after = bids
        .validator_bid(&ACCOUNT_1_PUBLIC_KEY)
        .expect("after bid should exist");

    assert_eq!(
        account_1_bid_after, account_1_bid_before,
        "bids before and after malicious attempt should be equal"
    );
}

#[ignore]
#[test]
fn regression_20210831_should_fail_to_undelegate_bid() {
    let mut builder = setup();

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        builder.get_auction_contract_hash(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => *BID_AMOUNT,
            auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
        },
    )
    .build();

    let delegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *ACCOUNT_1_ADDR,
        builder.get_auction_contract_hash(),
        auction::METHOD_DELEGATE,
        runtime_args! {
            auction::ARG_VALIDATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_DELEGATOR => ACCOUNT_1_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
        },
    )
    .build();

    builder.exec(add_bid_request).expect_success().commit();
    builder.exec(delegate_request).expect_success().commit();

    let bids = builder.get_bids();
    let default_account_bid_before = bids
        .validator_bid(&DEFAULT_ACCOUNT_PUBLIC_KEY)
        .expect("should have bid");
    assert_eq!(
        builder.get_purse_balance(*default_account_bid_before.bonding_purse()),
        *BID_AMOUNT,
    );
    assert!(
        !default_account_bid_before.inactive(),
        "newly added bid should be active"
    );

    let sender = *ACCOUNT_2_ADDR;
    let undelegate_args = runtime_args! {
        auction::ARG_VALIDATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        auction::ARG_DELEGATOR => ACCOUNT_1_PUBLIC_KEY.clone(),
        auction::ARG_AMOUNT => *BID_AMOUNT,
    };

    // ACCOUNT_2 undelegates ACCOUNT_1 by a direct auction contract call
    let exec_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        sender,
        builder.get_auction_contract_hash(),
        auction::METHOD_UNDELEGATE,
        undelegate_args.clone(),
    )
    .build();

    builder.exec(exec_request_1).commit();

    let error_1 = builder
        .get_error()
        .expect("attempt 1 should raise invalid context");
    assert!(
        matches!(error_1, CoreError::Exec(ExecError::Revert(ApiError::AuctionError(error_code))) if error_code == auction::Error::InvalidContext as u8),
        "{:?}",
        error_1
    );

    // ACCOUNT_2 undelegates ACCOUNT_1 through a proxy
    let exec_request_2 = ExecuteRequestBuilder::contract_call_by_name(
        sender,
        CONTRACT_HASH_NAME,
        METHOD_UNDELEGATE_PROXY_CALL,
        undelegate_args,
    )
    .build();

    builder.exec(exec_request_2).commit();

    let error_2 = builder
        .get_error()
        .expect("attempt 2 should raise invalid context");
    assert!(
        matches!(error_2, CoreError::Exec(ExecError::Revert(ApiError::AuctionError(error_code))) if error_code == auction::Error::InvalidContext as u8),
        "{:?}",
        error_2
    );

    let bids = builder.get_bids();
    let default_account_bid_after = bids
        .validator_bid(&DEFAULT_ACCOUNT_PUBLIC_KEY)
        .expect("should have bid");

    assert_eq!(
        default_account_bid_after, default_account_bid_before,
        "bids before and after malicious attempt should be equal"
    );
}

#[ignore]
#[test]
fn regression_20210831_should_fail_to_activate_bid() {
    let mut builder = setup();

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        builder.get_auction_contract_hash(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => *BID_AMOUNT,
            auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
        },
    )
    .build();

    builder.exec(add_bid_request).expect_success().commit();

    let bids = builder.get_bids();
    let bid = bids
        .validator_bid(&DEFAULT_ACCOUNT_PUBLIC_KEY)
        .expect("should have bid");
    assert!(!bid.inactive());

    let withdraw_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        builder.get_auction_contract_hash(),
        auction::METHOD_WITHDRAW_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => *BID_AMOUNT,
        },
    )
    .build();

    builder.exec(withdraw_bid_request).expect_success().commit();

    let bids = builder.get_bids();
    let bid = bids.validator_bid(&DEFAULT_ACCOUNT_PUBLIC_KEY);
    assert!(bid.is_none());

    let sender = *ACCOUNT_2_ADDR;
    let activate_bid_args = runtime_args! {
        auction::ARG_VALIDATOR_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
    };

    let activate_bid_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        sender,
        builder.get_auction_contract_hash(),
        auction::METHOD_ACTIVATE_BID,
        activate_bid_args.clone(),
    )
    .build();

    builder.exec(activate_bid_request_1);

    let error_1 = builder
        .get_error()
        .expect("attempt 1 should raise invalid context");
    assert!(
        matches!(error_1, CoreError::Exec(ExecError::Revert(ApiError::AuctionError(error_code))) if error_code == auction::Error::InvalidContext as u8),
        "{:?}",
        error_1
    );

    // ACCOUNT_2 unbonds ACCOUNT_1 through a proxy
    let activate_bid_request_2 = ExecuteRequestBuilder::contract_call_by_name(
        sender,
        CONTRACT_HASH_NAME,
        METHOD_ACTIVATE_BID_CALL,
        activate_bid_args,
    )
    .build();

    builder.exec(activate_bid_request_2).commit();

    let error_2 = builder
        .get_error()
        .expect("attempt 2 should raise invalid context");
    assert!(
        matches!(error_2, CoreError::Exec(ExecError::Revert(ApiError::AuctionError(error_code))) if error_code == auction::Error::InvalidContext as u8),
        "{:?}",
        error_2
    );
}
