use casper_engine_test_support::{
    internal::{
        ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_PUBLIC_KEY,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};

use casper_types::{
    runtime_args,
    system::auction::{self, DelegationRate},
    RuntimeArgs, U512,
};

const LARGE_DELEGATION_RATE: DelegationRate = 101;

#[ignore]
#[test]
fn should_run_ee_1174_delegation_rate_too_high() {
    let bid_amount = U512::one();

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let auction = builder.get_auction_contract_hash();

    let args = runtime_args! {
        auction::ARG_PUBLIC_KEY => *DEFAULT_ACCOUNT_PUBLIC_KEY,
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

    let _error = builder
        .get_exec_results()
        .last()
        .expect("should have results")
        .get(0)
        .expect("should have first result")
        .as_error()
        .expect("should have error");
}
