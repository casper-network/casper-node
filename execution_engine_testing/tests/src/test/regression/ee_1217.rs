use casper_engine_test_support::{
    internal::{
        ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_PUBLIC_KEY,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::core::{engine_state, execution};
use casper_types::{
    runtime_args, system::auction, ApiError, PublicKey, RuntimeArgs, SecretKey, U512,
};
use once_cell::sync::Lazy;

const CONTRACT_REGRESSION: &str = "ee_1217_regression.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";

const PACKAGE_NAME: &str = "call_auction";
const CONTRACT_ADD_BID_ENTRYPOINT_SESSION: &str = "add_bid_session";
const CONTRACT_ADD_BID_ENTRYPOINT_CONTRACT: &str = "add_bid_contract";
const CONTRACT_WITHDRAW_BID_ENTRYPOINT_SESSION: &str = "withdraw_bid_session";
const CONTRACT_WITHDRAW_BID_ENTRYPOINT_CONTRACT: &str = "withdraw_bid_contract";
const CONTRACT_DELEGATE_ENTRYPOINT_SESSION: &str = "delegate_session";
const CONTRACT_DELEGATE_ENTRYPOINT_CONTRACT: &str = "delegate_contract";
const CONTRACT_UNDELEGATE_ENTRYPOINT_SESSION: &str = "undelegate_session";
const CONTRACT_UNDELEGATE_ENTRYPOINT_CONTRACT: &str = "undelegate_contract";

static VALIDATOR_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([33; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});

#[ignore]
#[test]
fn should_fail_to_add_bid_from_stored_session_code() {
    let default_public_key_arg = DEFAULT_ACCOUNT_PUBLIC_KEY.clone();

    let store_call_auction_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_REGRESSION,
        runtime_args! {},
    )
    .build();

    let add_bid_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_NAME,
        None,
        CONTRACT_ADD_BID_ENTRYPOINT_SESSION,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => default_public_key_arg,
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder
        .exec(store_call_auction_request)
        .commit()
        .expect_success();

    builder.exec(add_bid_request);

    match builder.get_error() {
        None => panic!("should have returned an error"),
        Some(engine_state::Error::Exec(execution::Error::Revert(ApiError::AuctionError(14)))) => {}
        Some(error) => panic!("unexpected error: {:?}", error),
    }
}

#[ignore]
#[test]
fn should_add_bid_from_stored_contract_code() {
    let default_public_key_arg = DEFAULT_ACCOUNT_PUBLIC_KEY.clone();

    let store_call_auction_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_REGRESSION,
        runtime_args! {},
    )
    .build();

    let add_bid_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_NAME,
        None,
        CONTRACT_ADD_BID_ENTRYPOINT_CONTRACT,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => default_public_key_arg,
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder
        .exec(store_call_auction_request)
        .commit()
        .expect_success();

    builder.exec(add_bid_request).commit().expect_success();
}

#[ignore]
#[test]
fn should_fail_to_withdraw_bid_from_stored_session_code() {
    let default_public_key_arg = DEFAULT_ACCOUNT_PUBLIC_KEY.clone();

    let add_bid_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            auction::ARG_AMOUNT => U512::one(), // zero results in Error::BondTooSmall
            auction::ARG_PUBLIC_KEY => default_public_key_arg.clone(),
            auction::ARG_DELEGATION_RATE => 0u8,
        },
    )
    .build();

    let store_call_auction_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_REGRESSION,
        runtime_args! {},
    )
    .build();

    let withdraw_bid_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_NAME,
        None,
        CONTRACT_WITHDRAW_BID_ENTRYPOINT_SESSION,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => default_public_key_arg,
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(add_bid_request).commit().expect_success();

    builder
        .exec(store_call_auction_request)
        .commit()
        .expect_success();

    builder.exec(withdraw_bid_request);

    match builder.get_error() {
        None => panic!("should have returned an error"),
        Some(engine_state::Error::Exec(execution::Error::Revert(ApiError::AuctionError(14)))) => {}
        Some(error) => panic!("unexpected error: {:?}", error),
    }
}

#[ignore]
#[test]
fn should_withdraw_bid_from_stored_contract_code() {
    let default_public_key_arg = DEFAULT_ACCOUNT_PUBLIC_KEY.clone();

    let add_bid_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            auction::ARG_AMOUNT => U512::one(), // zero results in Error::BondTooSmall
            auction::ARG_PUBLIC_KEY => default_public_key_arg.clone(),
            auction::ARG_DELEGATION_RATE => 0u8,
        },
    )
    .build();

    let store_call_auction_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_REGRESSION,
        runtime_args! {},
    )
    .build();

    let withdraw_bid_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_NAME,
        None,
        CONTRACT_WITHDRAW_BID_ENTRYPOINT_CONTRACT,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => default_public_key_arg,
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder.exec(add_bid_request).commit().expect_success();

    builder
        .exec(store_call_auction_request)
        .commit()
        .expect_success();

    builder.exec(withdraw_bid_request).commit().expect_success();
}

#[ignore]
#[test]
fn should_fail_to_delegate_from_stored_session_code() {
    let default_public_key_arg = DEFAULT_ACCOUNT_PUBLIC_KEY.clone();

    let validator_public_key_arg = VALIDATOR_PUBLIC_KEY.clone();
    let validator_addr = VALIDATOR_PUBLIC_KEY.to_account_hash();

    let validator_fund_request = {
        const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
        const ARG_AMOUNT: &str = "amount";
        const ARG_TARGET: &str = "target";

        ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CONTRACT_TRANSFER_TO_ACCOUNT,
            runtime_args! {
                ARG_TARGET => validator_addr,
                ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
            },
        )
        .build()
    };

    let add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_PUBLIC_KEY.to_account_hash(),
        CONTRACT_ADD_BID,
        runtime_args! {
            auction::ARG_AMOUNT => U512::one(), // zero results in Error::BondTooSmall
            auction::ARG_PUBLIC_KEY => validator_public_key_arg.clone(),
            auction::ARG_DELEGATION_RATE => 0u8,
        },
    )
    .build();

    let store_call_auction_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_REGRESSION,
        runtime_args! {},
    )
    .build();

    let delegate_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_NAME,
        None,
        CONTRACT_DELEGATE_ENTRYPOINT_SESSION,
        runtime_args! {
            auction::ARG_DELEGATOR => default_public_key_arg,
            auction::ARG_VALIDATOR => validator_public_key_arg,
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder
        .exec(validator_fund_request)
        .commit()
        .expect_success();

    builder.exec(add_bid_request).commit().expect_success();

    builder
        .exec(store_call_auction_request)
        .commit()
        .expect_success();

    builder.exec(delegate_request);

    match builder.get_error() {
        None => panic!("should have returned an error"),
        Some(engine_state::Error::Exec(execution::Error::Revert(ApiError::AuctionError(14)))) => {}
        Some(error) => panic!("unexpected error: {:?}", error),
    }
}

#[ignore]
#[test]
fn should_delegate_from_stored_contract_code() {
    let default_public_key_arg = DEFAULT_ACCOUNT_PUBLIC_KEY.clone();

    let validator_public_key_arg = VALIDATOR_PUBLIC_KEY.clone();
    let validator_addr = VALIDATOR_PUBLIC_KEY.to_account_hash();

    let validator_fund_request = {
        const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
        const ARG_AMOUNT: &str = "amount";
        const ARG_TARGET: &str = "target";

        ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CONTRACT_TRANSFER_TO_ACCOUNT,
            runtime_args! {
                ARG_TARGET => validator_addr,
                ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
            },
        )
        .build()
    };

    let add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_PUBLIC_KEY.to_account_hash(),
        CONTRACT_ADD_BID,
        runtime_args! {
            auction::ARG_AMOUNT => U512::one(), // zero results in Error::BondTooSmall
            auction::ARG_PUBLIC_KEY => validator_public_key_arg.clone(),
            auction::ARG_DELEGATION_RATE => 0u8,
        },
    )
    .build();

    let store_call_auction_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_REGRESSION,
        runtime_args! {},
    )
    .build();

    let delegate_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_NAME,
        None,
        CONTRACT_DELEGATE_ENTRYPOINT_CONTRACT,
        runtime_args! {
            auction::ARG_DELEGATOR => default_public_key_arg,
            auction::ARG_VALIDATOR => validator_public_key_arg,
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder
        .exec(validator_fund_request)
        .commit()
        .expect_success();

    builder.exec(add_bid_request).commit().expect_success();

    builder
        .exec(store_call_auction_request)
        .commit()
        .expect_success();

    builder.exec(delegate_request).commit().expect_success();
}

#[ignore]
#[test]
fn should_fail_to_undelegate_from_stored_session_code() {
    let default_public_key_arg = DEFAULT_ACCOUNT_PUBLIC_KEY.clone();

    let validator_public_key_arg = VALIDATOR_PUBLIC_KEY.clone();
    let validator_addr = VALIDATOR_PUBLIC_KEY.to_account_hash();

    let validator_fund_request = {
        const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
        const ARG_AMOUNT: &str = "amount";
        const ARG_TARGET: &str = "target";

        ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CONTRACT_TRANSFER_TO_ACCOUNT,
            runtime_args! {
                ARG_TARGET => validator_addr,
                ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
            },
        )
        .build()
    };

    let add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_PUBLIC_KEY.to_account_hash(),
        CONTRACT_ADD_BID,
        runtime_args! {
            auction::ARG_AMOUNT => U512::one(), // zero results in Error::BondTooSmall
            auction::ARG_PUBLIC_KEY => validator_public_key_arg.clone(),
            auction::ARG_DELEGATION_RATE => 0u8,
        },
    )
    .build();

    let store_call_auction_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_REGRESSION,
        runtime_args! {},
    )
    .build();

    let delegate_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_NAME,
        None,
        CONTRACT_DELEGATE_ENTRYPOINT_CONTRACT,
        runtime_args! {
            auction::ARG_DELEGATOR => default_public_key_arg.clone(),
            auction::ARG_VALIDATOR => validator_public_key_arg.clone(),
        },
    )
    .build();

    let undelegate_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_NAME,
        None,
        CONTRACT_UNDELEGATE_ENTRYPOINT_SESSION,
        runtime_args! {
            auction::ARG_DELEGATOR => default_public_key_arg,
            auction::ARG_VALIDATOR => validator_public_key_arg,
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder
        .exec(validator_fund_request)
        .commit()
        .expect_success();

    builder.exec(add_bid_request).commit().expect_success();

    builder
        .exec(store_call_auction_request)
        .commit()
        .expect_success();

    builder.exec(delegate_request).commit().expect_success();

    builder.exec(undelegate_request).commit();

    match builder.get_error() {
        None => panic!("should have returned an error"),
        Some(engine_state::Error::Exec(execution::Error::Revert(ApiError::AuctionError(14)))) => {}
        Some(error) => panic!("unexpected error: {:?}", error),
    }
}

#[ignore]
#[test]
fn should_undelegate_from_stored_contract_code() {
    let default_public_key_arg = DEFAULT_ACCOUNT_PUBLIC_KEY.clone();

    let validator_public_key_arg = VALIDATOR_PUBLIC_KEY.clone();
    let validator_addr = VALIDATOR_PUBLIC_KEY.to_account_hash();

    let validator_fund_request = {
        const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
        const ARG_AMOUNT: &str = "amount";
        const ARG_TARGET: &str = "target";

        ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CONTRACT_TRANSFER_TO_ACCOUNT,
            runtime_args! {
                ARG_TARGET => validator_addr,
                ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
            },
        )
        .build()
    };

    let add_bid_request = ExecuteRequestBuilder::standard(
        VALIDATOR_PUBLIC_KEY.to_account_hash(),
        CONTRACT_ADD_BID,
        runtime_args! {
            auction::ARG_AMOUNT => U512::one(), // zero results in Error::BondTooSmall
            auction::ARG_PUBLIC_KEY => validator_public_key_arg.clone(),
            auction::ARG_DELEGATION_RATE => 0u8,
        },
    )
    .build();

    let store_call_auction_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_REGRESSION,
        runtime_args! {},
    )
    .build();

    let delegate_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_NAME,
        None,
        CONTRACT_DELEGATE_ENTRYPOINT_CONTRACT,
        runtime_args! {
            auction::ARG_DELEGATOR => default_public_key_arg.clone(),
            auction::ARG_VALIDATOR => validator_public_key_arg.clone(),
        },
    )
    .build();

    let undelegate_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_NAME,
        None,
        CONTRACT_UNDELEGATE_ENTRYPOINT_CONTRACT,
        runtime_args! {
            auction::ARG_DELEGATOR => default_public_key_arg,
            auction::ARG_VALIDATOR => validator_public_key_arg,
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    builder
        .exec(validator_fund_request)
        .commit()
        .expect_success();

    builder.exec(add_bid_request).commit().expect_success();

    builder
        .exec(store_call_auction_request)
        .commit()
        .expect_success();

    builder.exec(delegate_request).commit().expect_success();

    builder.exec(undelegate_request).commit().expect_success();
}
