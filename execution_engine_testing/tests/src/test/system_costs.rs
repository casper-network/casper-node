use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    utils, ChainspecConfig, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder,
    UpgradeRequestBuilder, DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
    DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_MAX_ASSOCIATED_KEYS, DEFAULT_MINIMUM_DELEGATION_AMOUNT,
    DEFAULT_PAYMENT, DEFAULT_PROTOCOL_VERSION, LOCAL_GENESIS_REQUEST,
    MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_types::{
    runtime_args,
    system::{
        auction::{self, DelegationRate},
        handle_payment, mint, AUCTION,
    },
    AuctionCosts, BrTableCost, ControlFlowCosts, CoreConfig, EraId, Gas, GenesisAccount,
    GenesisValidator, HandlePaymentCosts, HostFunction, HostFunctionCost, HostFunctionCosts,
    MessageLimits, MintCosts, Motes, OpcodeCosts, ProtocolVersion, PublicKey, RuntimeArgs,
    SecretKey, StandardPaymentCosts, StorageCosts, SystemConfig, WasmConfig, WasmV1Config,
    DEFAULT_ADD_BID_COST, DEFAULT_MINIMUM_BID_AMOUNT, DEFAULT_V1_MAX_STACK_HEIGHT,
    DEFAULT_V1_WASM_MAX_MEMORY, U512,
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
const BOND_AMOUNT: u64 = DEFAULT_MINIMUM_BID_AMOUNT + 42;
const BID_AMOUNT: u64 = 99 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const TRANSFER_AMOUNT: u64 = 123;
const BID_DELEGATION_RATE: DelegationRate = auction::DELEGATION_RATE_DENOMINATOR;
const UPDATED_CALL_CONTRACT_COST: HostFunctionCost = 12_345;
const NEW_ADD_BID_COST: u64 = 2_500_000_000;
const NEW_WITHDRAW_BID_COST: u32 = 2_500_000_000;
const NEW_DELEGATE_COST: u32 = 2_500_000_000;
const NEW_UNDELEGATE_COST: u32 = NEW_DELEGATE_COST;
const NEW_REDELEGATE_COST: u32 = NEW_DELEGATE_COST;
const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);

const OLD_PROTOCOL_VERSION: ProtocolVersion = DEFAULT_PROTOCOL_VERSION;
const NEW_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::from_parts(
    OLD_PROTOCOL_VERSION.value().major,
    OLD_PROTOCOL_VERSION.value().minor,
    OLD_PROTOCOL_VERSION.value().patch + 1,
);

const ARG_PURSE_NAME: &str = "purse_name";
const NAMED_PURSE_NAME: &str = "purse_1";
const ARG_AMOUNT: &str = "amount";

#[ignore]
#[test]
fn add_bid_and_withdraw_bid_have_expected_costs() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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

    let entity = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        entity
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_entity_hash_addr()
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

    let balance_before = builder.get_purse_balance(entity.main_purse());

    let proposer_reward_starting_balance_1 = builder.get_proposer_purse_balance();

    builder.exec(add_bid_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(entity.main_purse());

    let transaction_fee_1 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_1;

    let system_config = builder.chainspec().system_costs_config;
    let expected_call_cost = U512::from(system_config.auction_costs().add_bid);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BOND_AMOUNT) - transaction_fee_1
    );
    assert_eq!(builder.last_exec_gas_consumed().value(), expected_call_cost);

    // Withdraw bid
    let withdraw_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        entity
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_entity_hash_addr()
            .unwrap()
            .into(),
        auction::METHOD_WITHDRAW_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => U512::from(BOND_AMOUNT),
        },
    )
    .build();

    let balance_before = builder.get_purse_balance(entity.main_purse());

    let proposer_reward_starting_balance_2 = builder.get_proposer_purse_balance();

    builder.exec(withdraw_bid_request).expect_success().commit();

    let balance_after = builder.get_purse_balance(entity.main_purse());

    let transaction_fee_2 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_2;

    let system_config = builder.chainspec().system_costs_config;
    let expected_call_cost = U512::from(system_config.auction_costs().withdraw_bid);
    assert_eq!(balance_after, balance_before - transaction_fee_2);
    assert_eq!(builder.last_exec_gas_consumed().value(), expected_call_cost);
}

#[ignore]
#[test]
fn upgraded_add_bid_and_withdraw_bid_have_expected_costs() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade(&mut upgrade_request);

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
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_entity_hash_addr()
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

    builder.exec(add_bid_request).expect_success().commit();

    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_1 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_1;

    let expected_call_cost = U512::from(NEW_ADD_BID_COST);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BOND_AMOUNT) - transaction_fee_1
    );
    assert_eq!(builder.last_exec_gas_consumed().value(), expected_call_cost);

    // Withdraw bid
    let withdraw_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_entity_hash_addr()
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

    builder.exec(withdraw_bid_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_2 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_2;

    let call_cost = U512::from(NEW_WITHDRAW_BID_COST);
    assert_eq!(balance_after, balance_before - transaction_fee_2);
    assert_eq!(builder.last_exec_gas_consumed().value(), call_cost);
}

#[ignore]
#[test]
fn delegate_and_undelegate_have_expected_costs() {
    let mut builder = LmdbWasmTestBuilder::default();
    let accounts = {
        let validator_1 = GenesisAccount::account(
            VALIDATOR_1.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE),
                DelegationRate::zero(),
            )),
        );
        let validator_2 = GenesisAccount::account(
            VALIDATOR_2.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE),
                DelegationRate::zero(),
            )),
        );

        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(validator_1);
        tmp.push(validator_2);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    builder.run_genesis(run_genesis_request);

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
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let delegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_entity_hash_addr()
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

    builder.exec(delegate_request).expect_success().commit();

    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_1 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_1;

    let system_config = builder.chainspec().system_costs_config;
    let expected_call_cost = U512::from(system_config.auction_costs().delegate);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BID_AMOUNT) - transaction_fee_1,
    );
    assert_eq!(builder.last_exec_gas_consumed().value(), expected_call_cost);

    let redelegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_entity_hash_addr()
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

    builder.exec(redelegate_request).expect_success().commit();

    let system_config = builder.chainspec().system_costs_config;
    let expected_call_cost = U512::from(system_config.auction_costs().redelegate);
    assert_eq!(builder.last_exec_gas_consumed().value(), expected_call_cost);

    // Withdraw bid
    let undelegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_entity_hash_addr()
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

    builder.exec(undelegate_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_2 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_2;

    let system_config = builder.chainspec().system_costs_config;
    let expected_call_cost = U512::from(system_config.auction_costs().undelegate);
    assert_eq!(balance_after, balance_before - transaction_fee_2);
    assert_eq!(builder.last_exec_gas_consumed().value(), expected_call_cost);
}

#[ignore]
#[test]
fn upgraded_delegate_and_undelegate_have_expected_costs() {
    let mut builder = LmdbWasmTestBuilder::default();
    let accounts = {
        let validator_1 = GenesisAccount::account(
            VALIDATOR_1.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE),
                DelegationRate::zero(),
            )),
        );
        let validator_2 = GenesisAccount::account(
            VALIDATOR_2.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE),
                DelegationRate::zero(),
            )),
        );

        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(validator_1);
        tmp.push(validator_2);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    builder.run_genesis(run_genesis_request);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade(&mut upgrade_request);

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
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let delegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_entity_hash_addr()
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

    let proposer_reward_starting_balance_1 = builder.get_proposer_purse_balance();

    let balance_before = builder.get_purse_balance(account.main_purse());
    builder.exec(delegate_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_1 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_1;

    let call_cost = U512::from(NEW_DELEGATE_COST);
    assert_eq!(
        balance_after,
        balance_before - U512::from(BID_AMOUNT) - transaction_fee_1,
    );

    assert_eq!(builder.last_exec_gas_consumed().value(), call_cost);

    // Redelegate bid
    let redelegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_entity_hash_addr()
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

    builder.exec(redelegate_request).expect_success().commit();

    let expected_call_cost = U512::from(NEW_REDELEGATE_COST);
    assert_eq!(builder.last_exec_gas_consumed().value(), expected_call_cost);

    // Withdraw bid (undelegate)
    let undelegate_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        account
            .named_keys()
            .get(AUCTION)
            .unwrap()
            .into_entity_hash_addr()
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

    builder.exec(undelegate_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(account.main_purse());

    let transaction_fee_2 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_2;

    let call_cost = U512::from(NEW_UNDELEGATE_COST);
    assert_eq!(balance_after, balance_before - transaction_fee_2);
    assert_eq!(builder.last_exec_gas_consumed().value(), call_cost);
}

#[ignore]
#[test]
fn mint_transfer_has_expected_costs() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_NAMED_PURSE,
        runtime_args! {
            ARG_PURSE_NAME => NAMED_PURSE_NAME,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
        },
    )
    .build();

    builder.exec(transfer_request_1).expect_success().commit();

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    let purse_1 = default_account
        .named_keys()
        .get(NAMED_PURSE_NAME)
        .unwrap()
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

    builder.exec(transfer_request).expect_success().commit();
    let balance_after = builder.get_purse_balance(source);

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    assert_eq!(
        balance_after,
        balance_before - transfer_amount - transaction_fee,
    );
}

#[ignore]
#[test]
fn should_charge_for_erroneous_system_contract_calls() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let auction_hash = builder.get_auction_contract_hash();
    let mint_hash = builder.get_mint_contract_hash();
    let handle_payment_hash = builder.get_handle_payment_contract_hash();

    let system_config = builder.chainspec().system_costs_config;

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
            system_config.auction_costs().redelegate,
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
            system_config.mint_costs().mint.into(),
        ),
        (
            mint_hash,
            mint::METHOD_REDUCE_TOTAL_SUPPLY,
            system_config.mint_costs().reduce_total_supply.into(),
        ),
        (
            mint_hash,
            mint::METHOD_BALANCE,
            system_config.mint_costs().balance.into(),
        ),
        (
            mint_hash,
            mint::METHOD_TRANSFER,
            system_config.mint_costs().transfer.into(),
        ),
        (
            handle_payment_hash,
            handle_payment::METHOD_SET_REFUND_PURSE,
            system_config.handle_payment_costs().set_refund_purse.into(),
        ),
        // (
        //     handle_payment_hash,
        //     handle_payment::METHOD_FINALIZE_PAYMENT,
        //     system_config.handle_payment_costs().finalize_payment,
        // ),
        (
            auction_hash,
            "this_entrypoint_does_not_exists_1",
            system_config.no_such_entrypoint(),
        ),
        (
            mint_hash,
            "this_entrypoint_does_not_exists_2",
            system_config.no_such_entrypoint(),
        ),
        (
            handle_payment_hash,
            "this_entrypoint_does_not_exists_3",
            system_config.no_such_entrypoint(),
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

        builder.exec(exec_request).commit();

        let _error = builder
            .get_last_exec_result()
            .expect("should have results")
            .error()
            .cloned()
            .unwrap_or_else(|| panic!("should have error while executing {}", entrypoint));

        // assert!(matches!(
        //     error,
        //     Error::Exec(ExecError::NoSuchMethod(ref no_such_method)) if no_such_method ==
        // entrypoint), "{:?}",  error);

        let call_cost = U512::from(expected_cost);

        assert_eq!(
            builder.last_exec_gas_consumed().value(),
            call_cost,
            "{:?}",
            entrypoint
        );
    }
}

#[ignore]
#[test]
fn should_verify_do_nothing_charges_only_for_standard_payment() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let deploy_item = DeployItemBuilder::new()
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
        .with_standard_payment(runtime_args! {
            ARG_AMOUNT => *DEFAULT_PAYMENT
        })
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_deploy_hash([42; 32])
        .build();

    let do_nothing_request = ExecuteRequestBuilder::from_deploy_item(&deploy_item).build();

    let user_funds_before = builder.get_purse_balance(default_account.main_purse());

    let proposer_reward_starting_balance = builder.get_proposer_purse_balance();

    builder.exec(do_nothing_request).commit().expect_success();

    let user_funds_after = builder.get_purse_balance(default_account.main_purse());

    let transaction_fee = builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

    assert_eq!(user_funds_after, user_funds_before - transaction_fee,);

    assert_eq!(builder.last_exec_gas_consumed(), Gas::new(U512::zero()));
}

#[ignore]
#[test]
fn should_verify_wasm_add_bid_wasm_cost_is_not_recursive() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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
        control_flow: ControlFlowCosts {
            block: 0,
            op_loop: 0,
            op_if: 0,
            op_else: 0,
            end: 0,
            br: 0,
            br_if: 0,
            br_table: BrTableCost {
                cost: 0,
                size_multiplier: 0,
            },
            op_return: 0,
            call: 0,
            call_indirect: 0,
            drop: 0,
            select: 0,
        },
        integer_comparison: 0,
        conversion: 0,
        unreachable: 0,
        nop: 0,
        current_memory: 0,
        grow_memory: 0,
        sign: 0,
    };
    let new_storage_costs = StorageCosts::new(0);

    // We're elevating cost of `transfer_from_purse_to_purse` while zeroing others.
    // This will verify that user pays for the transfer host function _only_ while host does not
    // additionally charge for calling mint's "transfer" entrypoint under the hood.
    let new_host_function_costs = HostFunctionCosts {
        call_contract: HostFunction::fixed(UPDATED_CALL_CONTRACT_COST),
        ..Zero::zero()
    };
    let wasm_v1_config = WasmV1Config::new(
        DEFAULT_V1_WASM_MAX_MEMORY,
        DEFAULT_V1_MAX_STACK_HEIGHT,
        new_opcode_costs,
        new_host_function_costs,
    );
    let wasm_config = WasmConfig::new(MessageLimits::default(), wasm_v1_config);

    let new_max_associated_keys = DEFAULT_MAX_ASSOCIATED_KEYS;
    let new_auction_costs = AuctionCosts::default();
    let new_mint_costs = MintCosts {
        transfer: 0,
        ..Default::default()
    };
    let new_standard_payment_costs = StandardPaymentCosts::default();
    let new_handle_payment_costs = HandlePaymentCosts::default();

    let system_costs_config = SystemConfig::new(
        1,
        new_auction_costs,
        new_mint_costs,
        new_handle_payment_costs,
        new_standard_payment_costs,
    );

    let core_config = CoreConfig {
        max_associated_keys: new_max_associated_keys,
        ..Default::default()
    };

    let chainspec = ChainspecConfig {
        system_costs_config,
        wasm_config,
        core_config,
        storage_costs: new_storage_costs,
    };
    builder.with_chainspec(chainspec);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade(&mut upgrade_request);

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
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
    .build();

    // Verify that user is called and deploy raises runtime error
    let user_funds_before = builder.get_purse_balance(default_account.main_purse());

    let proposer_reward_starting_balance_1 = builder.get_proposer_purse_balance();

    builder.exec(add_bid_request).commit().expect_success();

    let user_funds_after = builder.get_purse_balance(default_account.main_purse());

    let transaction_fee_1 =
        builder.get_proposer_purse_balance() - proposer_reward_starting_balance_1;

    assert_eq!(
        user_funds_after,
        user_funds_before - transaction_fee_1 - U512::from(BOND_AMOUNT)
    );

    let expected_call_cost =
        U512::from(DEFAULT_ADD_BID_COST) + U512::from(UPDATED_CALL_CONTRACT_COST);

    assert_eq!(
        builder.last_exec_gas_consumed(),
        Gas::new(expected_call_cost)
    );
}
