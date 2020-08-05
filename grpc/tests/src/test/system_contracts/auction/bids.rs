use casperlabs_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    DEFAULT_ACCOUNT_ADDR,
};
use casperlabs_types::{
    account::AccountHash,
    auction::{ActiveBids, DelegationRate, ARG_AMOUNT, ARG_DELEGATION_RATE, METHOD_WITHDRAW_BID, METHOD_ADD_BID},
    bytesrepr::FromBytes,
    runtime_args, CLTyped, ContractHash, RuntimeArgs, U512,
};
const ARG_ENTRY_POINT: &str = "entry_point";

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_AUCTION_BIDS: &str = "auction_bids.wasm";
const TRANSFER_AMOUNT: u64 = 250_000_000 + 1000;
const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);

const ADD_BID_AMOUNT_1: u64 = 95_000;
const ADD_BID_DELEGATION_RATE_1: DelegationRate = 125;
const BID_AMOUNT_2: u64 = 5_000;
const ADD_BID_DELEGATION_RATE_2: DelegationRate = 126;

const WITHDRAW_BID_AMOUNT_2: u64 = 15_000;

fn get_value<T>(builder: &mut InMemoryWasmTestBuilder, contract_hash: ContractHash, name: &str) -> T
where
    T: FromBytes + CLTyped,
{
    let contract = builder
        .get_contract(contract_hash)
        .expect("should have contract");
    let key = contract
        .named_keys()
        .get(name)
        .expect("should have bid purses");
    let stored_value = builder.query(None, *key, &[]).expect("should query");
    let cl_value = stored_value
        .as_cl_value()
        .cloned()
        .expect("should be cl value");
    let result: T = cl_value.into_t().expect("should convert");
    result
}

#[ignore]
#[test]
fn should_run_add_bid() {
    let mut builder = InMemoryWasmTestBuilder::default();

    let exec_request = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);
    builder.exec(exec_request).commit().expect_success();

    let auction_hash = builder.get_auction_contract_hash();

    let auction_stored_value = builder
        .query(None, auction_hash.into(), &[])
        .expect("should query auction hash");
    let _auction = auction_stored_value
        .as_contract()
        .expect("should be contract");

    //
    let exec_request_1 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_ADD_BID,
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(exec_request_1).commit().expect_success();

    let active_bids: ActiveBids = get_value(&mut builder, auction_hash, "active_bids");

    assert_eq!(active_bids.len(), 1);

    let active_bid = active_bids.get(&DEFAULT_ACCOUNT_ADDR).unwrap();
    assert_eq!(
        builder.get_purse_balance(active_bid.bid_purse),
        U512::from(ADD_BID_AMOUNT_1)
    );
    assert_eq!(active_bid.delegation_rate, ADD_BID_DELEGATION_RATE_1);

    // 2nd bid top-up
    let exec_request_2 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_ADD_BID,
            ARG_AMOUNT => U512::from(BID_AMOUNT_2),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_2,
        },
    )
    .build();

    builder.exec(exec_request_2).commit().expect_success();

    let active_bids: ActiveBids = get_value(&mut builder, auction_hash, "active_bids");

    assert_eq!(active_bids.len(), 1);

    let active_bid = active_bids.get(&DEFAULT_ACCOUNT_ADDR).unwrap();
    assert_eq!(
        builder.get_purse_balance(active_bid.bid_purse),
        U512::from(ADD_BID_AMOUNT_1 + BID_AMOUNT_2)
    );
    assert_eq!(active_bid.delegation_rate, ADD_BID_DELEGATION_RATE_2);

    // 3. withdraw some amount
    let exec_request_3 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_WITHDRAW_BID,
            ARG_AMOUNT => U512::from(WITHDRAW_BID_AMOUNT_2),
        },
    )
    .build();
    builder.exec(exec_request_3).commit().expect_success();

    let active_bids: ActiveBids = get_value(&mut builder, auction_hash, "active_bids");

    assert_eq!(active_bids.len(), 1);

    let active_bid = active_bids.get(&DEFAULT_ACCOUNT_ADDR).unwrap();
    assert_eq!(
        builder.get_purse_balance(active_bid.bid_purse),
        U512::from(ADD_BID_AMOUNT_1 + BID_AMOUNT_2 - WITHDRAW_BID_AMOUNT_2)
    );
}
