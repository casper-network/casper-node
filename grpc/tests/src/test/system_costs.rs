use once_cell::sync::Lazy;

use casper_engine_test_support::{
    internal::{
        utils, ExecuteRequestBuilder, InMemoryWasmTestBuilder, UpgradeRequestBuilder,
        DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_PROTOCOL_VERSION,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    AccountHash, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
};
use casper_execution_engine::{
    core::engine_state::{upgrade::ActivationPoint, GenesisAccount},
    shared::{
        motes::Motes,
        system_config::{
            auction_costs::{
                AuctionCosts, DEFAULT_ADD_BID_COST, DEFAULT_DELEGATE_COST, DEFAULT_DISTRIBUTE_COST,
                DEFAULT_RUN_AUCTION_COST, DEFAULT_SLASH_COST, DEFAULT_UNDELEGATE_COST,
                DEFAULT_WITHDRAW_BID_COST, DEFAULT_WITHDRAW_DELEGATOR_REWARD_COST,
                DEFAULT_WITHDRAW_VALIDATOR_REWARD_COST,
            },
            SystemConfig,
        },
    },
    storage::protocol_data::DEFAULT_WASMLESS_TRANSFER_COST,
};
use casper_types::{
    auction::{self, DelegationRate},
    runtime_args,
    system_contract_type::AUCTION,
    ProtocolVersion, PublicKey, RuntimeArgs, U512,
};

const SYSTEM_CONTRACT_HASHES_NAME: &str = "system_contract_hashes.wasm";
const VALIDATOR_1: PublicKey = PublicKey::Ed25519([3; 32]);
static VALIDATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| VALIDATOR_1.into());
const VALIDATOR_1_STAKE: u64 = 250_000;
const BOND_AMOUNT: u64 = 42;
const BID_AMOUNT: u64 = 99;

const DELEGATION_RATE: DelegationRate = 123;

const NEW_ADD_BID_COST: u32 = DEFAULT_ADD_BID_COST * 2;
const NEW_WITHDRAW_BID_COST: u32 = DEFAULT_WITHDRAW_BID_COST * 3;
const NEW_DELEGATE_COST: u32 = DEFAULT_DELEGATE_COST * 4;
const NEW_UNDELEGATE_COST: u32 = DEFAULT_UNDELEGATE_COST * 5;
const DEFAULT_ACTIVATION_POINT: ActivationPoint = 1;

static OLD_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| *DEFAULT_PROTOCOL_VERSION);
static NEW_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| ProtocolVersion::from_parts(
    OLD_PROTOCOL_VERSION.value().major,
    OLD_PROTOCOL_VERSION.value().minor,
    OLD_PROTOCOL_VERSION.value().patch + 1,
));

#[cfg(not(feature = "use-system-contracts"))]
#[ignore]
#[test]
fn should_verify_calling_auction_add_and_withdraw_bid_costs() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let system_contract_hashes_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        SYSTEM_CONTRACT_HASHES_NAME,
        RuntimeArgs::default(),
    )
    .build();
    builder
        .exec(system_contract_hashes_request)
        .expect_success()
        .commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
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
    builder.exec(add_bid_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let call_cost = U512::from(DEFAULT_ADD_BID_COST);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BOND_AMOUNT) - call_cost
    );
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);

    // Withdraw bid
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

#[ignore]
#[test]
fn should_observe_upgraded_add_and_withdraw_bid_call_cost() {
    let new_wasmless_transfer_cost = DEFAULT_WASMLESS_TRANSFER_COST;

    let new_auction_costs = AuctionCosts {
        add_bid: NEW_ADD_BID_COST,
        withdraw_bid: NEW_WITHDRAW_BID_COST,
        ..Default::default()
    };

    let new_system_config = SystemConfig::new(new_wasmless_transfer_cost, new_auction_costs);

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_new_system_config(new_system_config)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let system_contract_hashes_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        SYSTEM_CONTRACT_HASHES_NAME,
        RuntimeArgs::default(),
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();
    builder
        .exec(system_contract_hashes_request)
        .expect_success()
        .commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
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
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(add_bid_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let call_cost = U512::from(NEW_ADD_BID_COST);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BOND_AMOUNT) - call_cost
    );
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);

    // Withdraw bid
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
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(withdraw_bid_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let call_cost = U512::from(NEW_WITHDRAW_BID_COST);
    assert_eq!(balance_after, balance_before - call_cost);
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);
}

#[cfg(not(feature = "use-system-contracts"))]
#[ignore]
#[test]
fn should_verify_calling_auction_delegate_and_undelegate_costs() {
    let mut builder = InMemoryWasmTestBuilder::default();
    let accounts = {
        let validator_1 = GenesisAccount::new(
            VALIDATOR_1,
            *VALIDATOR_1_ADDR,
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Motes::new(VALIDATOR_1_STAKE.into()),
        );

        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(validator_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    builder.run_genesis(&run_genesis_request);

    let system_contract_hashes_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        SYSTEM_CONTRACT_HASHES_NAME,
        RuntimeArgs::default(),
    )
    .build();
    builder
        .exec(system_contract_hashes_request)
        .expect_success()
        .commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let source_purse = account.main_purse();

    let delegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_hash()
            .unwrap(),
        auction::METHOD_DELEGATE,
        runtime_args! {
            auction::ARG_DELEGATOR => *DEFAULT_ACCOUNT_PUBLIC_KEY,
            auction::ARG_VALIDATOR => VALIDATOR_1,
            auction::ARG_SOURCE_PURSE => source_purse,
            auction::ARG_AMOUNT => U512::from(BID_AMOUNT),
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(delegate_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let call_cost = U512::from(DEFAULT_DELEGATE_COST);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BID_AMOUNT) - call_cost
    );
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);

    // Withdraw bid
    let undelegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_hash()
            .unwrap(),
        auction::METHOD_UNDELEGATE,
        runtime_args! {
            auction::ARG_DELEGATOR => *DEFAULT_ACCOUNT_PUBLIC_KEY,
            auction::ARG_VALIDATOR => VALIDATOR_1,
            auction::ARG_AMOUNT => U512::from(BID_AMOUNT),
            auction::ARG_UNBOND_PURSE => source_purse,
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(undelegate_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let call_cost = U512::from(DEFAULT_UNDELEGATE_COST);
    assert_eq!(balance_after, balance_before - call_cost);
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);
}

#[cfg(not(feature = "use-system-contracts"))]
#[ignore]
#[test]
fn should_observe_upgraded_delegate_and_undelegate_call_cost() {
    let new_wasmless_transfer_cost = DEFAULT_WASMLESS_TRANSFER_COST;

    let new_auction_costs = AuctionCosts {
        delegate: NEW_DELEGATE_COST,
        undelegate: NEW_UNDELEGATE_COST,
        ..Default::default()
    };

    let new_system_config = SystemConfig::new(new_wasmless_transfer_cost, new_auction_costs);

    let mut builder = InMemoryWasmTestBuilder::default();
    let accounts = {
        let validator_1 = GenesisAccount::new(
            VALIDATOR_1,
            *VALIDATOR_1_ADDR,
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Motes::new(VALIDATOR_1_STAKE.into()),
        );

        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(validator_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    builder.run_genesis(&run_genesis_request);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_new_system_config(new_system_config)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let system_contract_hashes_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        SYSTEM_CONTRACT_HASHES_NAME,
        RuntimeArgs::default(),
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();
    builder
        .exec(system_contract_hashes_request)
        .expect_success()
        .commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let source_purse = account.main_purse();

    let delegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_hash()
            .unwrap(),
        auction::METHOD_DELEGATE,
        runtime_args! {
            auction::ARG_DELEGATOR => *DEFAULT_ACCOUNT_PUBLIC_KEY,
            auction::ARG_VALIDATOR => VALIDATOR_1,
            auction::ARG_SOURCE_PURSE => source_purse,
            auction::ARG_AMOUNT => U512::from(BID_AMOUNT),
        },
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(delegate_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let call_cost = U512::from(NEW_DELEGATE_COST);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BID_AMOUNT) - call_cost
    );
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);

    // Withdraw bid
    let undelegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_hash()
            .unwrap(),
        auction::METHOD_UNDELEGATE,
        runtime_args! {
            auction::ARG_DELEGATOR => *DEFAULT_ACCOUNT_PUBLIC_KEY,
            auction::ARG_VALIDATOR => VALIDATOR_1,
            auction::ARG_AMOUNT => U512::from(BID_AMOUNT),
            auction::ARG_UNBOND_PURSE => source_purse,
        },
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(undelegate_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let call_cost = U512::from(NEW_UNDELEGATE_COST);
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

    // Entrypoints that could fail early due to missing arguments
    let entrypoint_calls = vec![
        (auction_hash, auction::METHOD_ADD_BID, DEFAULT_ADD_BID_COST),
        (
            auction_hash,
            auction::METHOD_WITHDRAW_BID,
            DEFAULT_WITHDRAW_BID_COST,
        ),
        (
            auction_hash,
            auction::METHOD_DELEGATE,
            DEFAULT_DELEGATE_COST,
        ),
        (
            auction_hash,
            auction::METHOD_UNDELEGATE,
            DEFAULT_UNDELEGATE_COST,
        ),
        (
            auction_hash,
            auction::METHOD_RUN_AUCTION,
            DEFAULT_RUN_AUCTION_COST,
        ),
        (auction_hash, auction::METHOD_SLASH, DEFAULT_SLASH_COST),
        (
            auction_hash,
            auction::METHOD_DISTRIBUTE,
            DEFAULT_DISTRIBUTE_COST,
        ),
        (
            auction_hash,
            auction::METHOD_WITHDRAW_DELEGATOR_REWARD,
            DEFAULT_WITHDRAW_DELEGATOR_REWARD_COST,
        ),
        (
            auction_hash,
            auction::METHOD_WITHDRAW_VALIDATOR_REWARD,
            DEFAULT_WITHDRAW_VALIDATOR_REWARD_COST,
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
            .unwrap_or_else(|| panic!("should have error while executing {}", entrypoint));

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
