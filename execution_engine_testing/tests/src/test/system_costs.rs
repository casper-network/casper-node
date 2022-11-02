use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    utils, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, UpgradeRequestBuilder,
    DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
    DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_MAX_ASSOCIATED_KEYS, DEFAULT_PAYMENT,
    DEFAULT_PROTOCOL_VERSION, MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    core::engine_state::{
        engine_config::{
            DEFAULT_MINIMUM_DELEGATION_AMOUNT, DEFAULT_STRICT_ARGUMENT_CHECKING,
            DEFAULT_VESTING_SCHEDULE_LENGTH_MILLIS,
        },
        genesis::GenesisValidator,
        EngineConfig, GenesisAccount, DEFAULT_MAX_QUERY_DEPTH,
        DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
    },
    shared::{
        host_function_costs::{Cost, HostFunction, HostFunctionCosts},
        opcode_costs::OpcodeCosts,
        storage_costs::StorageCosts,
        system_config::{
            auction_costs::{
                AuctionCosts, DEFAULT_ADD_BID_COST, DEFAULT_DELEGATE_COST, DEFAULT_UNDELEGATE_COST,
                DEFAULT_WITHDRAW_BID_COST,
            },
            handle_payment_costs::HandlePaymentCosts,
            mint_costs::{MintCosts, DEFAULT_TRANSFER_COST},
            standard_payment_costs::StandardPaymentCosts,
            SystemConfig, DEFAULT_WASMLESS_TRANSFER_COST,
        },
        wasm_config::{WasmConfig, DEFAULT_MAX_STACK_HEIGHT, DEFAULT_WASM_MAX_MEMORY},
    },
};
use casper_types::{
    runtime_args,
    system::{
        auction::{self, DelegationRate},
        handle_payment, mint, AUCTION,
    },
    EraId, Gas, Motes, ProtocolVersion, PublicKey, RuntimeArgs, SecretKey, U512,
};

use crate::wasm_utils;

const SYSTEM_CONTRACT_HASHES_NAME: &str = "system_contract_hashes.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";
const CONTRACT_TRANSFER_TO_NAMED_PURSE: &str = "transfer_to_named_purse.wasm";

static VALIDATOR_1_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::ed25519_from_bytes([123; SecretKey::ED25519_LENGTH]).unwrap());
static VALIDATOR_1: Lazy<PublicKey> = Lazy::new(|| PublicKey::from(&*VALIDATOR_1_SECRET_KEY));
const VALIDATOR_1_STAKE: u64 = 250_000;
static VALIDATOR_2_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::ed25519_from_bytes([124; SecretKey::ED25519_LENGTH]).unwrap());
static VALIDATOR_2: Lazy<PublicKey> = Lazy::new(|| PublicKey::from(&*VALIDATOR_2_SECRET_KEY));
const BOND_AMOUNT: u64 = 42;
const BID_AMOUNT: u64 = 99 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const TRANSFER_AMOUNT: u64 = 123;
const BID_DELEGATION_RATE: DelegationRate = auction::DELEGATION_RATE_DENOMINATOR;
const UPDATED_CALL_CONTRACT_COST: Cost = 12_345;
const NEW_ADD_BID_COST: u32 = DEFAULT_ADD_BID_COST * 2;
const NEW_WITHDRAW_BID_COST: u32 = DEFAULT_WITHDRAW_BID_COST * 3;
const NEW_DELEGATE_COST: u32 = DEFAULT_DELEGATE_COST * 4;
const NEW_UNDELEGATE_COST: u32 = DEFAULT_UNDELEGATE_COST * 5;
const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);

static OLD_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| *DEFAULT_PROTOCOL_VERSION);
static NEW_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| {
    ProtocolVersion::from_parts(
        OLD_PROTOCOL_VERSION.value().major,
        OLD_PROTOCOL_VERSION.value().minor,
        OLD_PROTOCOL_VERSION.value().patch + 1,
    )
});

const ARG_PURSE_NAME: &str = "purse_name";
const NAMED_PURSE_NAME: &str = "purse_1";
const ARG_AMOUNT: &str = "amount";

#[ignore]
#[test]
fn add_bid_and_withdraw_bid_have_expected_costs() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

    let system_contract_hashes_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        SYSTEM_CONTRACT_HASHES_NAME,
        RuntimeArgs::default(),
    )
    .build();
    builder
        .exec(system_contract_hashes_request)
        .expect_success()
        .apply()
        .commit_to_disk();

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
            .unwrap()
            .into(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => U512::from(BOND_AMOUNT),
            auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());

    let proposer_reward_starting_balance_1 = builder.get_proposer_purse_balance();

    builder
        .exec(add_bid_request)
        .expect_success()
        .apply()
        .commit_to_disk();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_1 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_1;

    let expected_call_cost = U512::from(
        builder
            .get_engine_state()
            .config()
            .system_config()
            .auction_costs()
            .add_bid,
    );
    assert_eq!(
        balance_after,
        balance_before - U512::from(BOND_AMOUNT) - transaction_fee_1
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
            .unwrap()
            .into(),
        auction::METHOD_WITHDRAW_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => U512::from(BOND_AMOUNT),
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());

    let proposer_reward_starting_balance_2 = builder.get_proposer_purse_balance();

    builder
        .exec(withdraw_bid_request)
        .expect_success()
        .apply()
        .commit_to_disk();

    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_2 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_2;

    let expected_call_cost = U512::from(
        builder
            .get_engine_state()
            .config()
            .system_config()
            .auction_costs()
            .withdraw_bid,
    );
    assert_eq!(balance_after, balance_before - transaction_fee_2);
    assert_eq!(builder.last_exec_gas_cost().value(), expected_call_cost);
}

#[ignore]
#[test]
fn upgraded_add_bid_and_withdraw_bid_have_expected_costs() {
    let new_wasmless_transfer_cost = DEFAULT_WASMLESS_TRANSFER_COST;
    let new_max_associated_keys = DEFAULT_MAX_ASSOCIATED_KEYS;

    let new_auction_costs = AuctionCosts {
        add_bid: NEW_ADD_BID_COST,
        withdraw_bid: NEW_WITHDRAW_BID_COST,
        ..Default::default()
    };
    let new_mint_costs = MintCosts::default();
    let new_standard_payment_costs = StandardPaymentCosts::default();
    let new_handle_payment_costs = HandlePaymentCosts::default();

    let new_system_config = SystemConfig::new(
        new_wasmless_transfer_cost,
        new_auction_costs,
        new_mint_costs,
        new_handle_payment_costs,
        new_standard_payment_costs,
    );

    let new_engine_config = EngineConfig::new(
        DEFAULT_MAX_QUERY_DEPTH,
        new_max_associated_keys,
        DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
        DEFAULT_MINIMUM_DELEGATION_AMOUNT,
        DEFAULT_STRICT_ARGUMENT_CHECKING,
        DEFAULT_VESTING_SCHEDULE_LENGTH_MILLIS,
        WasmConfig::default(),
        new_system_config,
    );

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade_with_upgrade_request(new_engine_config, &mut upgrade_request);

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
        .apply()
        .commit_to_disk();

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
            .unwrap()
            .into(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => U512::from(BOND_AMOUNT),
            auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
        },
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());

    let proposer_reward_starting_balance_1 = builder.get_proposer_purse_balance();

    builder
        .exec(add_bid_request)
        .expect_success()
        .apply()
        .commit_to_disk();

    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_1 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_1;

    let expected_call_cost = U512::from(NEW_ADD_BID_COST);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BOND_AMOUNT) - transaction_fee_1
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
            .unwrap()
            .into(),
        auction::METHOD_WITHDRAW_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => U512::from(BOND_AMOUNT),
        },
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());

    let proposer_reward_starting_balance_2 = builder.get_proposer_purse_balance();

    builder
        .exec(withdraw_bid_request)
        .expect_success()
        .apply()
        .commit_to_disk();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_2 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_2;

    let call_cost = U512::from(NEW_WITHDRAW_BID_COST);
    assert_eq!(balance_after, balance_before - transaction_fee_2);
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);
}

#[ignore]
#[test]
fn delegate_and_undelegate_have_expected_costs() {
    let mut builder = LmdbWasmTestBuilder::default();
    let accounts = {
        let validator_1 = GenesisAccount::account(
            VALIDATOR_1.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE.into()),
                DelegationRate::zero(),
            )),
        );
        let validator_2 = GenesisAccount::account(
            VALIDATOR_2.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE.into()),
                DelegationRate::zero(),
            )),
        );

        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(validator_1);
        tmp.push(validator_2);
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
        .apply()
        .commit_to_disk();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let delegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_hash()
            .unwrap()
            .into(),
        auction::METHOD_DELEGATE,
        runtime_args! {
            auction::ARG_DELEGATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_VALIDATOR => VALIDATOR_1.clone(),
            auction::ARG_AMOUNT => U512::from(BID_AMOUNT),
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());

    let proposer_reward_starting_balance_1 = builder.get_proposer_purse_balance();

    builder
        .exec(delegate_request)
        .expect_success()
        .apply()
        .commit_to_disk();

    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_1 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_1;

    let expected_call_cost = U512::from(
        builder
            .get_engine_state()
            .config()
            .system_config()
            .auction_costs()
            .delegate,
    );
    assert_eq!(
        balance_after,
        balance_before - U512::from(BID_AMOUNT) - transaction_fee_1,
    );
    assert_eq!(builder.last_exec_gas_cost().value(), expected_call_cost);

    let redelegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_hash()
            .unwrap()
            .into(),
        auction::METHOD_REDELEGATE,
        runtime_args! {
            auction::ARG_DELEGATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_VALIDATOR => VALIDATOR_1.clone(),
            auction::ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            auction::ARG_NEW_VALIDATOR => VALIDATOR_2.clone()
        },
    )
    .build();

    builder
        .exec(redelegate_request)
        .expect_success()
        .apply()
        .commit_to_disk();

    let expected_call_cost = U512::from(
        builder
            .get_engine_state()
            .config()
            .system_config()
            .auction_costs()
            .undelegate,
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
            .unwrap()
            .into(),
        auction::METHOD_UNDELEGATE,
        runtime_args! {
            auction::ARG_DELEGATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_VALIDATOR => VALIDATOR_1.clone(),
            auction::ARG_AMOUNT => U512::from(BID_AMOUNT - DEFAULT_MINIMUM_DELEGATION_AMOUNT),
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());

    let proposer_reward_starting_balance_2 = builder.get_proposer_purse_balance();

    builder
        .exec(undelegate_request)
        .expect_success()
        .apply()
        .commit_to_disk();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_2 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_2;

    let expected_call_cost = U512::from(
        builder
            .get_engine_state()
            .config()
            .system_config()
            .auction_costs()
            .undelegate,
    );
    assert_eq!(balance_after, balance_before - transaction_fee_2);
    assert_eq!(builder.last_exec_gas_cost().value(), expected_call_cost);
}

#[ignore]
#[test]
fn upgraded_delegate_and_undelegate_have_expected_costs() {
    let new_wasmless_transfer_cost = DEFAULT_WASMLESS_TRANSFER_COST;
    let new_max_associated_keys = DEFAULT_MAX_ASSOCIATED_KEYS;

    let new_auction_costs = AuctionCosts {
        delegate: NEW_DELEGATE_COST,
        undelegate: NEW_UNDELEGATE_COST,
        ..Default::default()
    };
    let new_mint_costs = MintCosts::default();
    let new_standard_payment_costs = StandardPaymentCosts::default();
    let new_handle_payment_costs = HandlePaymentCosts::default();

    let new_system_config = SystemConfig::new(
        new_wasmless_transfer_cost,
        new_auction_costs,
        new_mint_costs,
        new_handle_payment_costs,
        new_standard_payment_costs,
    );

    let new_engine_config = EngineConfig::new(
        DEFAULT_MAX_QUERY_DEPTH,
        new_max_associated_keys,
        DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
        DEFAULT_MINIMUM_DELEGATION_AMOUNT,
        DEFAULT_STRICT_ARGUMENT_CHECKING,
        DEFAULT_VESTING_SCHEDULE_LENGTH_MILLIS,
        WasmConfig::default(),
        new_system_config,
    );

    let mut builder = LmdbWasmTestBuilder::default();
    let accounts = {
        let validator_1 = GenesisAccount::account(
            VALIDATOR_1.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE.into()),
                DelegationRate::zero(),
            )),
        );
        let validator_2 = GenesisAccount::account(
            VALIDATOR_2.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE.into()),
                DelegationRate::zero(),
            )),
        );

        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(validator_1);
        tmp.push(validator_2);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    builder.run_genesis(&run_genesis_request);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade_with_upgrade_request(new_engine_config, &mut upgrade_request);

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
        .apply()
        .commit_to_disk();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let delegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_hash()
            .unwrap()
            .into(),
        auction::METHOD_DELEGATE,
        runtime_args! {
            auction::ARG_DELEGATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_VALIDATOR => VALIDATOR_1.clone(),
            auction::ARG_AMOUNT => U512::from(BID_AMOUNT),
        },
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    let proposer_reward_starting_balance_1 = builder.get_proposer_purse_balance();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder
        .exec(delegate_request)
        .expect_success()
        .apply()
        .commit_to_disk();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_1 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_1;

    let call_cost = U512::from(NEW_DELEGATE_COST);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BID_AMOUNT) - transaction_fee_1,
    );
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);

    let redelegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_hash()
            .unwrap()
            .into(),
        auction::METHOD_REDELEGATE,
        runtime_args! {
            auction::ARG_DELEGATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_VALIDATOR => VALIDATOR_1.clone(),
            auction::ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            auction::ARG_NEW_VALIDATOR => VALIDATOR_2.clone()
        },
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    builder
        .exec(redelegate_request)
        .expect_success()
        .apply()
        .commit_to_disk();

    let expected_call_cost = U512::from(NEW_UNDELEGATE_COST);
    assert_eq!(builder.last_exec_gas_cost().value(), expected_call_cost);

    // Withdraw bid
    let undelegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_hash()
            .unwrap()
            .into(),
        auction::METHOD_UNDELEGATE,
        runtime_args! {
            auction::ARG_DELEGATOR => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_VALIDATOR => VALIDATOR_1.clone(),
            auction::ARG_AMOUNT => U512::from(BID_AMOUNT - DEFAULT_MINIMUM_DELEGATION_AMOUNT),
        },
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    let balance_before = builder.get_purse_balance(account.main_purse());

    let proposer_reward_starting_balance_2 = builder.get_proposer_purse_balance();

    builder
        .exec(undelegate_request)
        .expect_success()
        .apply()
        .commit_to_disk();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_2 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_2;

    let call_cost = U512::from(NEW_UNDELEGATE_COST);
    assert_eq!(balance_after, balance_before - transaction_fee_2);
    assert_eq!(builder.last_exec_gas_cost().value(), call_cost);
}

#[ignore]
#[test]
fn mint_transfer_has_expected_costs() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_NAMED_PURSE,
        runtime_args! {
            ARG_PURSE_NAME => NAMED_PURSE_NAME,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
        },
    )
    .build();

    builder
        .exec(transfer_request_1)
        .expect_success()
        .apply()
        .commit_to_disk();

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let purse_1 = default_account.named_keys()[NAMED_PURSE_NAME]
        .into_uref()
        .expect("should have purse");

    let mint_hash = builder.get_mint_contract_hash();

    let source = default_account.main_purse();
    let target = purse_1;

    let id = Some(0u64);

    let transfer_amount = U512::from(TRANSFER_AMOUNT);

    let transfer_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        mint_hash,
        mint::METHOD_TRANSFER,
        runtime_args! {
            mint::ARG_TO => Some(*DEFAULT_ACCOUNT_ADDR),
            mint::ARG_SOURCE => source,
            mint::ARG_TARGET => target,
            mint::ARG_AMOUNT => U512::from(TRANSFER_AMOUNT),
            mint::ARG_ID => id,
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(source);

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder
        .exec(transfer_request)
        .expect_success()
        .apply()
        .commit_to_disk();
    let balance_after = builder.get_purse_balance(source);

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    let expected_call_cost = U512::from(DEFAULT_TRANSFER_COST);
    assert_eq!(
        balance_after,
        balance_before - transfer_amount - transaction_fee,
    );
    assert_eq!(builder.last_exec_gas_cost().value(), expected_call_cost);
}

#[ignore]
#[test]
fn should_charge_for_erroneous_system_contract_calls() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

    let auction_hash = builder.get_auction_contract_hash();
    let mint_hash = builder.get_mint_contract_hash();
    let handle_payment_hash = builder.get_handle_payment_contract_hash();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let system_config = *builder.get_engine_state().config().system_config();

    // Entrypoints that could fail early due to missing arguments
    let entrypoint_calls = vec![
        (
            auction_hash,
            auction::METHOD_ADD_BID,
            system_config.auction_costs().add_bid,
        ),
        (
            auction_hash,
            auction::METHOD_WITHDRAW_BID,
            system_config.auction_costs().withdraw_bid,
        ),
        (
            auction_hash,
            auction::METHOD_DELEGATE,
            system_config.auction_costs().delegate,
        ),
        (
            auction_hash,
            auction::METHOD_UNDELEGATE,
            system_config.auction_costs().undelegate,
        ),
        (
            auction_hash,
            auction::METHOD_REDELEGATE,
            // We currently charge the same for both entrypoints.
            system_config.auction_costs().undelegate,
        ),
        (
            auction_hash,
            auction::METHOD_RUN_AUCTION,
            system_config.auction_costs().run_auction,
        ),
        (
            auction_hash,
            auction::METHOD_SLASH,
            system_config.auction_costs().slash,
        ),
        (
            auction_hash,
            auction::METHOD_DISTRIBUTE,
            system_config.auction_costs().distribute,
        ),
        (
            mint_hash,
            mint::METHOD_MINT,
            system_config.mint_costs().mint,
        ),
        (
            mint_hash,
            mint::METHOD_REDUCE_TOTAL_SUPPLY,
            system_config.mint_costs().reduce_total_supply,
        ),
        (
            mint_hash,
            mint::METHOD_BALANCE,
            system_config.mint_costs().balance,
        ),
        (
            mint_hash,
            mint::METHOD_TRANSFER,
            system_config.mint_costs().transfer,
        ),
        (
            handle_payment_hash,
            handle_payment::METHOD_SET_REFUND_PURSE,
            system_config.handle_payment_costs().set_refund_purse,
        ),
        (
            handle_payment_hash,
            handle_payment::METHOD_FINALIZE_PAYMENT,
            system_config.handle_payment_costs().finalize_payment,
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

        let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

        builder.exec(exec_request).apply().commit_to_disk();

        let _error = builder
            .get_last_exec_results()
            .expect("should have results")
            .get(0)
            .expect("should have first result")
            .as_error()
            .unwrap_or_else(|| panic!("should have error while executing {}", entrypoint));

        let transaction_fee =
            builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

        let balance_after = builder.get_purse_balance(account.main_purse());

        let call_cost = U512::from(expected_cost);
        assert_eq!(
            balance_after,
            balance_before - transaction_fee,
            "Calling a failed entrypoint {} does not incur expected cost of {}",
            entrypoint,
            expected_cost,
        );
        assert_eq!(builder.last_exec_gas_cost().value(), call_cost);
    }
}

#[ignore]
#[test]
fn should_verify_do_nothing_charges_only_for_standard_payment() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let do_nothing_request = {
        let deploy_item = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT
            })
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash([42; 32])
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    };

    let user_funds_before = builder.get_purse_balance(default_account.main_purse());

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder
        .exec(do_nothing_request)
        .apply()
        .commit_to_disk()
        .expect_success();

    let user_funds_after = builder.get_purse_balance(default_account.main_purse());

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    assert_eq!(user_funds_after, user_funds_before - transaction_fee,);

    assert_eq!(builder.last_exec_gas_cost(), Gas::new(U512::zero()));
}

#[ignore]
#[test]
fn should_verify_wasm_add_bid_wasm_cost_is_not_recursive() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(&*PRODUCTION_RUN_GENESIS_REQUEST);

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
        integer_comparison: 0,
        conversion: 0,
        unreachable: 0,
        nop: 0,
        current_memory: 0,
        grow_memory: 0,
        regular: 0,
    };
    let new_storage_costs = StorageCosts::new(0);

    // We're elevating cost of `transfer_from_purse_to_purse` while zeroing others.
    // This will verify that user pays for the transfer host function _only_ while host does not
    // additionally charge for calling mint's "transfer" entrypoint under the hood.
    let new_host_function_costs = HostFunctionCosts {
        read_value: HostFunction::fixed(0),
        dictionary_get: HostFunction::fixed(0),
        write: HostFunction::fixed(0),
        dictionary_put: HostFunction::fixed(0),
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
        transfer_from_purse_to_purse: HostFunction::fixed(0),
        get_balance: HostFunction::fixed(0),
        get_phase: HostFunction::fixed(0),
        get_system_contract: HostFunction::fixed(0),
        get_main_purse: HostFunction::fixed(0),
        read_host_buffer: HostFunction::fixed(0),
        create_contract_package_at_hash: HostFunction::fixed(0),
        create_contract_user_group: HostFunction::fixed(0),
        add_contract_version: HostFunction::fixed(0),
        disable_contract_version: HostFunction::fixed(0),
        call_contract: HostFunction::fixed(UPDATED_CALL_CONTRACT_COST),
        call_versioned_contract: HostFunction::fixed(0),
        get_named_arg_size: HostFunction::fixed(0),
        get_named_arg: HostFunction::fixed(0),
        remove_contract_user_group: HostFunction::fixed(0),
        provision_contract_user_group_uref: HostFunction::fixed(0),
        remove_contract_user_group_urefs: HostFunction::fixed(0),
        print: HostFunction::fixed(0),
        blake2b: HostFunction::fixed(0),
        random_bytes: HostFunction::fixed(0),
    };

    let new_wasm_config = WasmConfig::new(
        DEFAULT_WASM_MAX_MEMORY,
        DEFAULT_MAX_STACK_HEIGHT,
        new_opcode_costs,
        new_storage_costs,
        new_host_function_costs,
    );

    let new_wasmless_transfer_cost = 0;
    let new_max_associated_keys = DEFAULT_MAX_ASSOCIATED_KEYS;
    let new_auction_costs = AuctionCosts::default();
    let new_mint_costs = MintCosts::default();
    let new_standard_payment_costs = StandardPaymentCosts::default();
    let new_handle_payment_costs = HandlePaymentCosts::default();

    let new_system_config = SystemConfig::new(
        new_wasmless_transfer_cost,
        new_auction_costs,
        new_mint_costs,
        new_handle_payment_costs,
        new_standard_payment_costs,
    );

    let new_engine_config = EngineConfig::new(
        DEFAULT_MAX_QUERY_DEPTH,
        new_max_associated_keys,
        DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
        DEFAULT_MINIMUM_DELEGATION_AMOUNT,
        DEFAULT_STRICT_ARGUMENT_CHECKING,
        DEFAULT_VESTING_SCHEDULE_LENGTH_MILLIS,
        new_wasm_config,
        new_system_config,
    );

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade_with_upgrade_request(new_engine_config, &mut upgrade_request);

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let add_bid_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => U512::from(BOND_AMOUNT),
            auction::ARG_DELEGATION_RATE => BID_DELEGATION_RATE,
        },
    )
    .with_protocol_version(*NEW_PROTOCOL_VERSION)
    .build();

    // Verify that user is called and deploy raises runtime error
    let user_funds_before = builder.get_purse_balance(default_account.main_purse());

    let proposer_reward_starting_balance_1 = builder.get_proposer_purse_balance();

    builder
        .exec(add_bid_request)
        .apply()
        .commit_to_disk()
        .expect_success();

    let user_funds_after = builder.get_purse_balance(default_account.main_purse());

    let transaction_fee_1 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_1;

    let expected_call_cost =
        U512::from(DEFAULT_ADD_BID_COST) + U512::from(UPDATED_CALL_CONTRACT_COST);

    assert_eq!(
        user_funds_after,
        user_funds_before - transaction_fee_1 - U512::from(BOND_AMOUNT)
    );

    assert_eq!(builder.last_exec_gas_cost(), Gas::new(expected_call_cost));
}
