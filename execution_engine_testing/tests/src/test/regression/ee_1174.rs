use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_PUBLIC_KEY,
    PRODUCTION_RUN_GENESIS_REQUEST,
};

use casper_execution_engine::{engine_state::Error, execution};
use casper_types::{
    runtime_args,
    system::{
        self,
        auction::{self, DelegationRate},
    },
    ApiError, U512,
};

const LARGE_DELEGATION_RATE: DelegationRate = 101;

#[ignore]
#[test]
fn should_run_ee_1174_delegation_rate_too_high() {
    let bid_amount = U512::one();

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let auction = builder.get_auction_contract_hash();

    let args = runtime_args! {
        auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        auction::ARG_AMOUNT => bid_amount,
        auction::ARG_DELEGATION_RATE => LARGE_DELEGATION_RATE,
    };

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        auction,
        auction::METHOD_ADD_BID,
        args,
    )
    .build();

    builder.exec(add_bid_request).commit();

    let error = builder
        .get_last_exec_result()
        .expect("should have results")
        .get(0)
        .expect("should have first result")
        .as_error()
        .cloned()
        .expect("should have error");

    assert!(matches!(
        error,
        Error::Exec(execution::Error::Revert(ApiError::AuctionError(auction_error))) if auction_error == system::auction::Error::DelegationRateTooLarge as u8));
}
