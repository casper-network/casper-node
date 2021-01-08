use casper_engine_test_support::{
    internal::{
        ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_PUBLIC_KEY,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::shared::system_config::auction_costs::{
    DEFAULT_ADD_BID_COST, DEFAULT_WITHDRAW_BID_COST,
};
use casper_types::{
    auction::{self, DelegationRate},
    runtime_args,
    system_contract_type::AUCTION,
    RuntimeArgs, U512,
};

const SYSTEM_CONTRACT_HASHES_NAME: &str = "system_contract_hashes.wasm";

#[cfg(not(feature = "use-system-contracts"))]
#[ignore]
#[test]
fn should_verify_calling_auction_add_and_withdraw_bid_costs() {
    const BOND_AMOUNT: u64 = 42;
    const DELEGATION_RATE: DelegationRate = 123;

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        SYSTEM_CONTRACT_HASHES_NAME,
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(exec_request).expect_success().commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_hash()
            .unwrap(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => *DEFAULT_ACCOUNT_PUBLIC_KEY,
            auction::ARG_SOURCE_PURSE => account.main_purse(),
            auction::ARG_AMOUNT => U512::from(BOND_AMOUNT),
            auction::ARG_DELEGATION_RATE => DELEGATION_RATE,
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(exec_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let call_cost = U512::from(DEFAULT_ADD_BID_COST);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BOND_AMOUNT) - call_cost
    );
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);

    //
    // Withdraw bid
    //

    let withdraw_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_hash()
            .unwrap(),
        auction::METHOD_WITHDRAW_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => *DEFAULT_ACCOUNT_PUBLIC_KEY,
            auction::ARG_AMOUNT => U512::from(BOND_AMOUNT),
            auction::ARG_UNBOND_PURSE => account.main_purse(),
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(withdraw_bid_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let call_cost = U512::from(DEFAULT_WITHDRAW_BID_COST);
    assert_eq!(balance_after, balance_before - call_cost);
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);
}

#[cfg(not(feature = "use-system-contracts"))]
#[ignore]
#[test]
fn should_charge_for_errorneous_system_contract_calls() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        SYSTEM_CONTRACT_HASHES_NAME,
        RuntimeArgs::default(),
    )
    .build();
    builder.exec(exec_request).expect_success().commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let auction_hash = account
        .named_keys()
        .get(AUCTION)
        .unwrap()
        .into_hash()
        .unwrap();

    let entrypoint_calls = vec![
        (
            auction_hash,
            auction::METHOD_ADD_BID,
            DEFAULT_ADD_BID_COST,
        ),
        (
            auction_hash,
            auction::METHOD_WITHDRAW_BID,
            DEFAULT_WITHDRAW_BID_COST,
        ),
    ];

    for (contract_hash, entrypoint, expected_cost) in entrypoint_calls {
        let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
            *DEFAULT_ACCOUNT_ADDR,
            contract_hash,
            entrypoint,
            RuntimeArgs::default(),
        )
        .build();

        let balance_before = builder.get_purse_balance(account.main_purse());

        builder.exec(exec_request).commit();

        let _error = builder
            .get_exec_responses()
            .last()
            .expect("should have results")
            .get(0)
            .expect("should have first result")
            .as_error()
            .expect("should have error");

        let balance_after = builder.get_purse_balance(account.main_purse());

        let call_cost = U512::from(DEFAULT_WITHDRAW_BID_COST);
        assert_eq!(
            balance_after,
            balance_before - call_cost,
            "Calling a failed entrypoint {} does not incur expected cost of {}",
            entrypoint,
            expected_cost
        );
        assert_eq!(builder.last_exec_gas_cost().value(), call_cost);
    }
}
