use casper_engine_test_support::{
    internal::{
        ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_PUBLIC_KEY,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casper_types::{runtime_args, system::auction, RuntimeArgs, U512};

const CONTRACT_REGRESSION: &str = "ee_1217_regression.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";

const PACKAGE_NAME: &str = "call_auction";
const CONTRACT_CALL_AUCTION_ENTRYPOINT: &str = "call_auction";

#[ignore]
#[test]
fn should_fail_to_call_auction_as_non_session_code() {
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

    let call_auction_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
        *DEFAULT_ACCOUNT_ADDR,
        PACKAGE_NAME,
        None,
        CONTRACT_CALL_AUCTION_ENTRYPOINT,
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

    builder.exec(call_auction_request);

    builder.expect_failure();
}
