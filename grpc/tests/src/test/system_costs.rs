use once_cell::sync::Lazy;

use casper_engine_test_support::{
    internal::{
        utils, DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder,
        UpgradeRequestBuilder, DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_PAYMENT,
        DEFAULT_PROTOCOL_VERSION, DEFAULT_RUN_GENESIS_REQUEST,
    },
    AccountHash, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
    MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::{
    core::engine_state::{upgrade::ActivationPoint, GenesisAccount, SYSTEM_ACCOUNT_ADDR},
    shared::{
        host_function_costs::{
            HostFunction, HostFunctionCosts, DEFAULT_TRANSFER_FROM_PURSE_TO_PURSE_COST,
        },
        motes::Motes,
        opcode_costs::OpcodeCosts,
        storage_costs::StorageCosts,
        system_config::{
            auction_costs::{
                AuctionCosts, DEFAULT_ADD_BID_COST, DEFAULT_DELEGATE_COST, DEFAULT_DISTRIBUTE_COST,
                DEFAULT_RUN_AUCTION_COST, DEFAULT_SLASH_COST, DEFAULT_UNDELEGATE_COST,
                DEFAULT_WITHDRAW_BID_COST, DEFAULT_WITHDRAW_DELEGATOR_REWARD_COST,
                DEFAULT_WITHDRAW_VALIDATOR_REWARD_COST,
            },
            mint_costs::{
                MintCosts, DEFAULT_BALANCE_COST, DEFAULT_MINT_COST,
                DEFAULT_REDUCE_TOTAL_SUPPLY_COST, DEFAULT_TRANSFER_COST,
            },
            proof_of_stake_costs::{
                ProofOfStakeCosts, DEFAULT_FINALIZE_PAYMENT_COST, DEFAULT_SET_REFUND_PURSE_COST,
            },
            standard_payment_costs::{StandardPaymentCosts, DEFAULT_PAY_COST},
            SystemConfig,
        },
        wasm,
        wasm_config::{WasmConfig, DEFAULT_MAX_STACK_HEIGHT, DEFAULT_WASM_MAX_MEMORY},
    },
    storage::protocol_data::DEFAULT_WASMLESS_TRANSFER_COST,
};
use casper_types::{
    auction::{self, DelegationRate},
    mint, proof_of_stake, runtime_args,
    system_contract_type::AUCTION,
    ProtocolVersion, PublicKey, RuntimeArgs, SecretKey, U512,
};

const SYSTEM_CONTRACT_HASHES_NAME: &str = "system_contract_hashes.wasm";
const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";

const VALIDATOR_1_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::ed25519([123; SecretKey::ED25519_LENGTH]));
const VALIDATOR_1: Lazy<PublicKey> = Lazy::new(|| PublicKey::from(&*VALIDATOR_1_SECRET_KEY));
static VALIDATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALIDATOR_1));
const VALIDATOR_1_STAKE: u64 = 250_000;
const BOND_AMOUNT: u64 = 42;
const BID_AMOUNT: u64 = 99;
const TRANSFER_AMOUNT: u64 = 123;
const BID_DELEGATION_RATE: DelegationRate = 123;

const NEW_ADD_BID_COST: u32 = DEFAULT_ADD_BID_COST * 2;
const NEW_WITHDRAW_BID_COST: u32 = DEFAULT_WITHDRAW_BID_COST * 3;
const NEW_DELEGATE_COST: u32 = DEFAULT_DELEGATE_COST * 4;
const NEW_UNDELEGATE_COST: u32 = DEFAULT_UNDELEGATE_COST * 5;
const DEFAULT_ACTIVATION_POINT: ActivationPoint = 1;

static OLD_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| *DEFAULT_PROTOCOL_VERSION);
static NEW_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| {
    ProtocolVersion::from_parts(
        OLD_PROTOCOL_VERSION.value().major,
        OLD_PROTOCOL_VERSION.value().minor,
        OLD_PROTOCOL_VERSION.value().patch + 1,
    )
});

const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";

#[ignore]
#[test]
fn add_bid_and_withdraw_bid_have_expected_costs() {
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
            auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(add_bid_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let expected_call_cost = U512::from(DEFAULT_PAY_COST) + U512::from(DEFAULT_ADD_BID_COST);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BOND_AMOUNT) - expected_call_cost
    );
    assert_eq!(builder.last_exec_gas_cost().value(), expected_call_cost);

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

    let expected_call_cost = U512::from(DEFAULT_PAY_COST) + U512::from(DEFAULT_WITHDRAW_BID_COST);
    assert_eq!(balance_after, balance_before - expected_call_cost);
    assert_eq!(builder.last_exec_gas_cost().value(), expected_call_cost);
}

#[ignore]
#[test]
fn upgraded_add_bid_and_withdraw_bid_have_expected_costs() {
    let new_wasmless_transfer_cost = DEFAULT_WASMLESS_TRANSFER_COST;

    let new_auction_costs = AuctionCosts {
        add_bid: NEW_ADD_BID_COST,
        withdraw_bid: NEW_WITHDRAW_BID_COST,
        ..Default::default()
    };
    let new_mint_costs = MintCosts::default();
    let new_standard_payment_costs = StandardPaymentCosts::default();
    let new_proof_of_stake_costs = ProofOfStakeCosts::default();

    let new_system_config = SystemConfig::new(
        new_wasmless_transfer_cost,
        new_auction_costs,
        new_mint_costs,
        new_proof_of_stake_costs,
        new_standard_payment_costs,
    );

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
            auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
        },
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(add_bid_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let expected_call_cost = U512::from(DEFAULT_PAY_COST) + U512::from(NEW_ADD_BID_COST);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BOND_AMOUNT) - expected_call_cost
    );
    assert_eq!(builder.last_exec_gas_cost().value(), expected_call_cost);

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

    let call_cost = U512::from(DEFAULT_PAY_COST) + U512::from(NEW_WITHDRAW_BID_COST);
    assert_eq!(balance_after, balance_before - call_cost);
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);
}

#[ignore]
#[test]
fn delegate_and_undelegate_have_expected_costs() {
    let mut builder = InMemoryWasmTestBuilder::default();
    let accounts = {
        let validator_1 = GenesisAccount::new(
            *VALIDATOR_1,
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
            auction::ARG_VALIDATOR => *VALIDATOR_1,
            auction::ARG_SOURCE_PURSE => source_purse,
            auction::ARG_AMOUNT => U512::from(BID_AMOUNT),
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(delegate_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let expected_call_cost = U512::from(DEFAULT_PAY_COST) + U512::from(DEFAULT_DELEGATE_COST);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BID_AMOUNT) - expected_call_cost
    );
    assert_eq!(builder.last_exec_gas_cost().value(), expected_call_cost);

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
            auction::ARG_VALIDATOR => *VALIDATOR_1,
            auction::ARG_AMOUNT => U512::from(BID_AMOUNT),
            auction::ARG_UNBOND_PURSE => source_purse,
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(undelegate_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let expected_call_cost = U512::from(DEFAULT_PAY_COST) + U512::from(DEFAULT_UNDELEGATE_COST);
    assert_eq!(balance_after, balance_before - expected_call_cost);
    assert_eq!(builder.last_exec_gas_cost().value(), expected_call_cost);
}

#[ignore]
#[test]
fn upgraded_delegate_and_undelegate_have_expected_costs() {
    let new_wasmless_transfer_cost = DEFAULT_WASMLESS_TRANSFER_COST;

    let new_auction_costs = AuctionCosts {
        delegate: NEW_DELEGATE_COST,
        undelegate: NEW_UNDELEGATE_COST,
        ..Default::default()
    };
    let new_mint_costs = MintCosts::default();
    let new_standard_payment_costs = StandardPaymentCosts::default();
    let new_proof_of_stake_costs = ProofOfStakeCosts::default();

    let new_system_config = SystemConfig::new(
        new_wasmless_transfer_cost,
        new_auction_costs,
        new_mint_costs,
        new_proof_of_stake_costs,
        new_standard_payment_costs,
    );

    let mut builder = InMemoryWasmTestBuilder::default();
    let accounts = {
        let validator_1 = GenesisAccount::new(
            *VALIDATOR_1,
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
            auction::ARG_VALIDATOR => *VALIDATOR_1,
            auction::ARG_SOURCE_PURSE => source_purse,
            auction::ARG_AMOUNT => U512::from(BID_AMOUNT),
        },
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(delegate_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let call_cost = U512::from(DEFAULT_PAY_COST) + U512::from(NEW_DELEGATE_COST);
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
            auction::ARG_VALIDATOR => *VALIDATOR_1,
            auction::ARG_AMOUNT => U512::from(BID_AMOUNT),
            auction::ARG_UNBOND_PURSE => source_purse,
        },
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(undelegate_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let call_cost = U512::from(DEFAULT_PAY_COST) + U512::from(NEW_UNDELEGATE_COST);
    assert_eq!(balance_after, balance_before - call_cost);
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);
}

#[ignore]
#[test]
fn mint_transfer_has_expected_costs() {
    let mut builder = InMemoryWasmTestBuilder::default();

    let accounts = {
        let validator_1 = GenesisAccount::new(
            *VALIDATOR_1,
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

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let validator_1_account = builder
        .get_account(*VALIDATOR_1_ADDR)
        .expect("should have account");

    let mint_hash = builder.get_mint_contract_hash();

    let source = default_account.main_purse();
    let target = validator_1_account.main_purse();

    let id = Some(0u64);

    let transfer_amount = U512::from(TRANSFER_AMOUNT);

    let transfer_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        mint_hash,
        mint::METHOD_TRANSFER,
        runtime_args! {
            mint::ARG_TO => Some(*VALIDATOR_1_ADDR),
            mint::ARG_SOURCE => source,
            mint::ARG_TARGET => target,
            mint::ARG_AMOUNT => U512::from(TRANSFER_AMOUNT),
            mint::ARG_ID => id,
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(source);
    builder.exec(transfer_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(source);

    let expected_call_cost = U512::from(DEFAULT_PAY_COST) + U512::from(DEFAULT_TRANSFER_COST);
    assert_eq!(
        balance_after,
        balance_before - transfer_amount - expected_call_cost
    );
    assert_eq!(builder.last_exec_gas_cost().value(), expected_call_cost);
}

#[ignore]
#[test]
fn should_charge_for_erroneous_system_contract_calls() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let auction_hash = builder.get_auction_contract_hash();
    let mint_hash = builder.get_mint_contract_hash();
    let pos_hash = builder.get_pos_contract_hash();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

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
        (mint_hash, mint::METHOD_MINT, DEFAULT_MINT_COST),
        (
            mint_hash,
            mint::METHOD_REDUCE_TOTAL_SUPPLY,
            DEFAULT_REDUCE_TOTAL_SUPPLY_COST,
        ),
        (mint_hash, mint::METHOD_BALANCE, DEFAULT_BALANCE_COST),
        (mint_hash, mint::METHOD_TRANSFER, DEFAULT_TRANSFER_COST),
        (
            pos_hash,
            proof_of_stake::METHOD_SET_REFUND_PURSE,
            DEFAULT_SET_REFUND_PURSE_COST,
        ),
        (
            pos_hash,
            proof_of_stake::METHOD_FINALIZE_PAYMENT,
            DEFAULT_FINALIZE_PAYMENT_COST,
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

        let call_cost = U512::from(DEFAULT_PAY_COST) + U512::from(expected_cost);
        let lhs = balance_after;
        let rhs = balance_before - call_cost;
        assert_eq!(
            lhs,
            rhs,
            "Calling a failed entrypoint {} does not incur expected cost of {} but {}",
            entrypoint,
            expected_cost,
            if lhs > rhs { lhs - rhs } else { rhs - lhs }
        );
        assert_eq!(builder.last_exec_gas_cost().value(), call_cost);
    }
}

#[ignore]
#[test]
fn should_not_charge_system_account_for_running_auction() {
    // This verifies a corner case where system account calls `run_auction` entrypoint, but it
    // shouldn't be charged for doing so. Otherwise if that happens the system could fail as in
    // production code system account is most likely empty.
    let fund_system_account_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! { ARG_TARGET => SYSTEM_ACCOUNT_ADDR, ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE) },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let auction_hash = builder.get_auction_contract_hash();

    builder
        .exec(fund_system_account_request)
        .commit()
        .expect_success();

    let system_account = builder
        .get_account(SYSTEM_ACCOUNT_ADDR)
        .expect("should have system account");
    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let run_auction_as_system_request = ExecuteRequestBuilder::contract_call_by_hash(
        SYSTEM_ACCOUNT_ADDR,
        auction_hash,
        auction::METHOD_RUN_AUCTION,
        RuntimeArgs::default(),
    )
    .build();

    let run_auction_as_user_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        auction_hash,
        auction::METHOD_RUN_AUCTION,
        RuntimeArgs::default(),
    )
    .build();

    // Verify that system account is not charged
    let system_funds_before = builder.get_purse_balance(system_account.main_purse());
    builder
        .exec(run_auction_as_system_request)
        .commit()
        .expect_success();
    let system_funds_after = builder.get_purse_balance(system_account.main_purse());

    assert_eq!(system_funds_before, system_funds_after);

    let user_funds_before = builder.get_purse_balance(default_account.main_purse());
    builder.exec(run_auction_as_user_request).commit();
    let user_funds_after = builder.get_purse_balance(default_account.main_purse());

    let expected_call_cost = U512::from(DEFAULT_PAY_COST) + U512::from(DEFAULT_RUN_AUCTION_COST);
    let lhs = user_funds_after;
    let rhs = user_funds_before - expected_call_cost;
    assert_eq!(
        lhs,
        rhs,
        "unexpected difference {}",
        if lhs > rhs { lhs - rhs } else { rhs - lhs },
    );
}

#[ignore]
#[test]
fn should_verify_do_nothing_charges_only_for_standard_payment() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let do_nothing_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_bytes(wasm::do_nothing_bytes(), RuntimeArgs::default())
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT
            })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    let user_funds_before = builder.get_purse_balance(default_account.main_purse());
    builder.exec(do_nothing_request).commit().expect_success();
    let user_funds_after = builder.get_purse_balance(default_account.main_purse());

    let expected_call_cost = U512::from(DEFAULT_PAY_COST);
    let lhs = user_funds_after;
    let rhs = user_funds_before - expected_call_cost;
    assert_eq!(
        lhs,
        rhs,
        "unexpected difference {}",
        if lhs > rhs { lhs - rhs } else { rhs - lhs },
    );
}

#[ignore]
#[test]
fn should_verify_wasm_add_bid_wasm_cost_is_not_recursive() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);

    // We're elevating cost of `transfer_from_purse_to_purse` while zeroing others.
    // This will verify that user pays for the transfer host function _only_ while host does not
    // additionally charge for calling mint's "transfer" entrypoint under the hood.
    let new_transfer_from_purse_to_purse_cost = DEFAULT_TRANSFER_FROM_PURSE_TO_PURSE_COST * 100;

    let new_opcode_costs = OpcodeCosts {
        bit: 0,
        add: 0,
        mul: 0,
        div: 0,
        load: 0,
        store: 0,
        op_const: 0,
        local: 0,
        global: 0,
        control_flow: 0,
        integer_comparsion: 0,
        conversion: 0,
        unreachable: 0,
        nop: 0,
        current_memory: 0,
        grow_memory: 0,
        regular: 0,
    };
    let new_storage_costs = StorageCosts::new(0);
    let new_host_function_costs = HostFunctionCosts {
        read_value: HostFunction::fixed(0),
        read_value_local: HostFunction::fixed(0),
        write: HostFunction::fixed(0),
        write_local: HostFunction::fixed(0),
        add: HostFunction::fixed(0),
        new_uref: HostFunction::fixed(0),
        load_named_keys: HostFunction::fixed(0),
        ret: HostFunction::fixed(0),
        get_key: HostFunction::fixed(0),
        has_key: HostFunction::fixed(0),
        put_key: HostFunction::fixed(0),
        remove_key: HostFunction::fixed(0),
        revert: HostFunction::fixed(0),
        is_valid_uref: HostFunction::fixed(0),
        add_associated_key: HostFunction::fixed(0),
        remove_associated_key: HostFunction::fixed(0),
        update_associated_key: HostFunction::fixed(0),
        set_action_threshold: HostFunction::fixed(0),
        get_caller: HostFunction::fixed(0),
        get_blocktime: HostFunction::fixed(0),
        create_purse: HostFunction::fixed(0),
        transfer_to_account: HostFunction::fixed(0),
        transfer_from_purse_to_account: HostFunction::fixed(0),
        transfer_from_purse_to_purse: HostFunction::fixed(new_transfer_from_purse_to_purse_cost),
        get_balance: HostFunction::fixed(0),
        get_phase: HostFunction::fixed(0),
        get_system_contract: HostFunction::fixed(0),
        get_main_purse: HostFunction::fixed(0),
        read_host_buffer: HostFunction::fixed(0),
        create_contract_package_at_hash: HostFunction::fixed(0),
        create_contract_user_group: HostFunction::fixed(0),
        add_contract_version: HostFunction::fixed(0),
        disable_contract_version: HostFunction::fixed(0),
        call_contract: HostFunction::fixed(0),
        call_versioned_contract: HostFunction::fixed(0),
        get_named_arg_size: HostFunction::fixed(0),
        get_named_arg: HostFunction::fixed(0),
        remove_contract_user_group: HostFunction::fixed(0),
        provision_contract_user_group_uref: HostFunction::fixed(0),
        remove_contract_user_group_urefs: HostFunction::fixed(0),
        print: HostFunction::fixed(0),
        blake2b: HostFunction::fixed(0),
    };

    let new_wasm_config = WasmConfig::new(
        DEFAULT_WASM_MAX_MEMORY,
        DEFAULT_MAX_STACK_HEIGHT,
        new_opcode_costs,
        new_storage_costs,
        new_host_function_costs,
    );

    let new_wasmless_transfer_cost = 0;
    let new_auction_costs = AuctionCosts::default();
    let new_mint_costs = MintCosts::default();
    let new_standard_payment_costs = StandardPaymentCosts::default();
    let new_proof_of_stake_costs = ProofOfStakeCosts::default();

    let new_system_config = SystemConfig::new(
        new_wasmless_transfer_cost,
        new_auction_costs,
        new_mint_costs,
        new_proof_of_stake_costs,
        new_standard_payment_costs,
    );

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_new_wasm_config(new_wasm_config)
            .with_new_system_config(new_system_config)
            .build()
    };

    builder.upgrade_with_upgrade_request(&mut upgrade_request);

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let add_bid_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => *DEFAULT_ACCOUNT_PUBLIC_KEY,
            auction::ARG_AMOUNT => U512::from(BOND_AMOUNT),
            auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
        },
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    // Verify that user is called and deploy raises runtime error
    let user_funds_before = builder.get_purse_balance(default_account.main_purse());
    builder.exec(add_bid_request).commit().expect_success();
    let user_funds_after = builder.get_purse_balance(default_account.main_purse());

    let expected_call_cost = U512::from(DEFAULT_PAY_COST)
        + U512::from(DEFAULT_ADD_BID_COST)
        + U512::from(new_transfer_from_purse_to_purse_cost)
        + U512::from(BOND_AMOUNT);
    let lhs = user_funds_after;
    let rhs = user_funds_before - expected_call_cost;
    assert_eq!(
        lhs,
        rhs,
        "unexpected difference {}",
        if lhs > rhs { lhs - rhs } else { rhs - lhs },
    );
}
