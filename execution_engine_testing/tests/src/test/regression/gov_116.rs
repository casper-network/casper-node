use std::{collections::BTreeSet, iter::FromIterator};

use casper_execution_engine::engine_state::{
    genesis::ExecConfigBuilder, EngineConfigBuilder, RunGenesisRequest,
};
use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_CHAINSPEC_REGISTRY, DEFAULT_GENESIS_CONFIG_HASH,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_PROTOCOL_VERSION,
    DEFAULT_VALIDATOR_SLOTS, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_types::{
    runtime_args,
    system::{
        auction::{self, DelegationRate, EraValidators, VESTING_SCHEDULE_LENGTH_MILLIS},
        mint,
    },
    GenesisAccount, GenesisValidator, Motes, PublicKey, SecretKey, U256, U512,
};

const MINIMUM_BONDED_AMOUNT: u64 = 1_000;

/// Validator with smallest stake will withdraw most of his stake to ensure we did move time forward
/// to unlock his whole vesting schedule.
const WITHDRAW_AMOUNT: u64 = MINIMUM_BONDED_AMOUNT - 1;

/// Initial lockup period
const VESTING_BASE: u64 = DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

const DAY_MILLIS: u64 = 24 * 60 * 60 * 1000;
const WEEK_MILLIS: u64 = 7 * DAY_MILLIS;
const DELEGATION_RATE: DelegationRate = 0;

/// Simplified vesting weeks for testing purposes. Each element is used as an argument to
/// run_auction call.
const VESTING_WEEKS: [u64; 3] = [
    // Passes the vesting schedule (aka initial lockup + schedule length)
    VESTING_BASE + VESTING_SCHEDULE_LENGTH_MILLIS,
    // One week after
    VESTING_BASE + VESTING_SCHEDULE_LENGTH_MILLIS + WEEK_MILLIS,
    // Two weeks after
    VESTING_BASE + VESTING_SCHEDULE_LENGTH_MILLIS + (2 * WEEK_MILLIS),
];

static GENESIS_VALIDATOR_PUBLIC_KEYS: Lazy<BTreeSet<PublicKey>> = Lazy::new(|| {
    let mut set = BTreeSet::new();
    for i in 1..=DEFAULT_VALIDATOR_SLOTS {
        let mut secret_key_bytes = [255u8; 32];
        U256::from(i).to_big_endian(&mut secret_key_bytes);
        let secret_key = SecretKey::secp256k1_from_bytes(secret_key_bytes).unwrap();
        let public_key = PublicKey::from(&secret_key);
        set.insert(public_key);
    }
    set
});

static GENESIS_VALIDATORS: Lazy<Vec<GenesisAccount>> = Lazy::new(|| {
    let mut vec = Vec::with_capacity(GENESIS_VALIDATOR_PUBLIC_KEYS.len());

    for (index, public_key) in GENESIS_VALIDATOR_PUBLIC_KEYS.iter().enumerate() {
        let account = GenesisAccount::account(
            public_key.clone(),
            Motes::new(U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)),
            Some(GenesisValidator::new(
                Motes::new(U512::from(index + 1) * 1_000),
                DelegationRate::zero(),
            )),
        );
        vec.push(account);
    }

    vec
});

static LOWEST_STAKE_VALIDATOR: Lazy<PublicKey> = Lazy::new(|| {
    let mut genesis_accounts: Vec<&GenesisAccount> = GENESIS_ACCOUNTS.iter().collect();
    genesis_accounts.sort_by_key(|genesis_account| genesis_account.staked_amount());

    // Finds a genesis validator with lowest stake
    let genesis_account = genesis_accounts
        .into_iter()
        .find(|genesis_account| {
            genesis_account.is_validator() && genesis_account.staked_amount() > Motes::zero()
        })
        .unwrap();

    assert_eq!(
        genesis_account.staked_amount(),
        Motes::new(U512::from(MINIMUM_BONDED_AMOUNT))
    );

    genesis_account.public_key()
});

static GENESIS_ACCOUNTS: Lazy<Vec<GenesisAccount>> = Lazy::new(|| {
    let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
    tmp.append(&mut GENESIS_VALIDATORS.clone());
    tmp
});

fn initialize_builder() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();

    let run_genesis_request = utils::create_run_genesis_request(GENESIS_ACCOUNTS.clone());
    builder.run_genesis(&run_genesis_request);

    let fund_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => PublicKey::System.to_account_hash(),
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    builder.exec(fund_request).expect_success().commit();

    builder
}

#[ignore]
#[test]
fn should_not_retain_genesis_validator_slot_protection_after_vesting_period_elapsed() {
    let lowest_stake_validator_addr = LOWEST_STAKE_VALIDATOR.to_account_hash();

    let mut builder = initialize_builder();

    // Unlock all funds of genesis validator
    builder.run_auction(VESTING_WEEKS[0], Vec::new());

    let era_validators_1: EraValidators = builder.get_era_validators();

    let (last_era_1, weights_1) = era_validators_1.iter().last().unwrap();
    let genesis_validator_stake_1 = weights_1.get(&LOWEST_STAKE_VALIDATOR).unwrap();
    let next_validator_set_1 = BTreeSet::from_iter(weights_1.keys().cloned());
    assert_eq!(
        next_validator_set_1,
        GENESIS_VALIDATOR_PUBLIC_KEYS.clone(),
        "expected validator set should be unchanged"
    );

    let withdraw_bid_request = {
        let auction_hash = builder.get_auction_contract_hash();
        let session_args = runtime_args! {
            auction::ARG_PUBLIC_KEY => LOWEST_STAKE_VALIDATOR.clone(),
            auction::ARG_AMOUNT => U512::from(WITHDRAW_AMOUNT),
        };
        ExecuteRequestBuilder::contract_call_by_hash(
            lowest_stake_validator_addr,
            auction_hash,
            auction::METHOD_WITHDRAW_BID,
            session_args,
        )
        .build()
    };

    builder.exec(withdraw_bid_request).expect_success().commit();

    builder.run_auction(VESTING_WEEKS[1], Vec::new());

    let era_validators_2: EraValidators = builder.get_era_validators();

    let (last_era_2, weights_2) = era_validators_2.iter().last().unwrap();
    assert!(last_era_2 > last_era_1);
    let genesis_validator_stake_2 = weights_2.get(&LOWEST_STAKE_VALIDATOR).unwrap();

    let next_validator_set_2 = BTreeSet::from_iter(weights_2.keys().cloned());
    assert_eq!(next_validator_set_2, GENESIS_VALIDATOR_PUBLIC_KEYS.clone());

    assert!(
        genesis_validator_stake_1 > genesis_validator_stake_2,
        "stake should decrease in future era"
    );

    let stake_diff = if genesis_validator_stake_1 > genesis_validator_stake_2 {
        genesis_validator_stake_1 - genesis_validator_stake_2
    } else {
        genesis_validator_stake_2 - genesis_validator_stake_1
    };

    assert_eq!(stake_diff, U512::from(WITHDRAW_AMOUNT));

    // Add nonfounding validator higher than `unbonding_account` has after unlocking & withdrawing

    // New validator bids with the original stake of unbonding_account to take his place in future
    // era. We know that unbonding_account has now smaller stake than before.

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        builder.get_auction_contract_hash(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => *genesis_validator_stake_1,
            auction::ARG_DELEGATION_RATE => DELEGATION_RATE,
        },
    )
    .build();

    builder.exec(add_bid_request).expect_success().commit();

    builder.run_auction(VESTING_WEEKS[2], Vec::new());

    let era_validators_3: EraValidators = builder.get_era_validators();
    let (last_era_3, weights_3) = era_validators_3.iter().last().unwrap();
    assert!(last_era_3 > last_era_2);

    assert_eq!(
        weights_3.len(),
        DEFAULT_VALIDATOR_SLOTS as usize,
        "auction incorrectly computed more than slots than available"
    );

    assert!(
        weights_3.contains_key(&*DEFAULT_ACCOUNT_PUBLIC_KEY),
        "new non-genesis validator should replace a genesis validator with smaller stake"
    );

    assert!(
        !weights_3.contains_key(&LOWEST_STAKE_VALIDATOR),
        "unbonded account should be out of the set"
    );

    let next_validator_set_3 = BTreeSet::from_iter(weights_3.keys().cloned());
    let expected_validators = {
        let mut pks = GENESIS_VALIDATOR_PUBLIC_KEYS.clone();
        pks.remove(&LOWEST_STAKE_VALIDATOR);
        pks.insert(DEFAULT_ACCOUNT_PUBLIC_KEY.clone());
        pks
    };
    assert_eq!(
        next_validator_set_3, expected_validators,
        "actual next validator set does not match expected validator set"
    );
}

#[ignore]
#[test]
fn should_retain_genesis_validator_slot_protection() {
    const CASPER_VESTING_SCHEDULE_PERIOD_MILLIS: u64 = 91 * DAY_MILLIS;
    const CASPER_LOCKED_FUNDS_PERIOD_MILLIS: u64 = 90 * DAY_MILLIS;
    const CASPER_VESTING_BASE: u64 =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + CASPER_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = {
        let engine_config = EngineConfigBuilder::default()
            .with_vesting_schedule_period_millis(CASPER_VESTING_SCHEDULE_PERIOD_MILLIS)
            .build();

        let run_genesis_request = {
            let accounts = GENESIS_ACCOUNTS.clone();
            let exec_config = ExecConfigBuilder::default()
                .with_accounts(accounts)
                .with_locked_funds_period_millis(CASPER_LOCKED_FUNDS_PERIOD_MILLIS)
                .build();

            RunGenesisRequest::new(
                *DEFAULT_GENESIS_CONFIG_HASH,
                *DEFAULT_PROTOCOL_VERSION,
                exec_config,
                DEFAULT_CHAINSPEC_REGISTRY.clone(),
            )
        };

        let mut builder = LmdbWasmTestBuilder::new_temporary_with_config(engine_config);
        builder.run_genesis(&run_genesis_request);

        let fund_request = ExecuteRequestBuilder::transfer(
            *DEFAULT_ACCOUNT_ADDR,
            runtime_args! {
                mint::ARG_TARGET => PublicKey::System.to_account_hash(),
                mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
                mint::ARG_ID => <Option<u64>>::None,
            },
        )
        .build();

        builder.exec(fund_request).expect_success().commit();

        builder
    };

    let era_validators_1: EraValidators = builder.get_era_validators();

    let (last_era_1, weights_1) = era_validators_1.iter().last().unwrap();
    let genesis_validator_stake_1 = weights_1.get(&LOWEST_STAKE_VALIDATOR).unwrap();
    // One higher than the lowest stake
    let winning_stake = *genesis_validator_stake_1 + U512::one();
    let next_validator_set_1 = BTreeSet::from_iter(weights_1.keys().cloned());
    assert_eq!(
        next_validator_set_1,
        GENESIS_VALIDATOR_PUBLIC_KEYS.clone(),
        "expected validator set should be unchanged"
    );

    builder.run_auction(CASPER_VESTING_BASE, Vec::new());

    let era_validators_2: EraValidators = builder.get_era_validators();

    let (last_era_2, weights_2) = era_validators_2.iter().last().unwrap();
    assert!(last_era_2 > last_era_1);
    let next_validator_set_2 = BTreeSet::from_iter(weights_2.keys().cloned());
    assert_eq!(next_validator_set_2, GENESIS_VALIDATOR_PUBLIC_KEYS.clone());

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        builder.get_auction_contract_hash(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => winning_stake,
            auction::ARG_DELEGATION_RATE => DELEGATION_RATE,
        },
    )
    .build();

    builder.exec(add_bid_request).expect_success().commit();

    builder.run_auction(CASPER_VESTING_BASE + WEEK_MILLIS, Vec::new());

    // All genesis validator slots are protected after ~1 week
    let era_validators_3: EraValidators = builder.get_era_validators();
    let (last_era_3, weights_3) = era_validators_3.iter().last().unwrap();
    assert!(last_era_3 > last_era_2);
    let next_validator_set_3 = BTreeSet::from_iter(weights_3.keys().cloned());
    assert_eq!(next_validator_set_3, GENESIS_VALIDATOR_PUBLIC_KEYS.clone());

    // After 13 weeks ~ 91 days lowest stake validator is dropped and replaced with higher bid
    builder.run_auction(
        CASPER_VESTING_BASE + VESTING_SCHEDULE_LENGTH_MILLIS,
        Vec::new(),
    );

    let era_validators_4: EraValidators = builder.get_era_validators();
    let (last_era_4, weights_4) = era_validators_4.iter().last().unwrap();
    assert!(last_era_4 > last_era_3);
    let next_validator_set_4 = BTreeSet::from_iter(weights_4.keys().cloned());
    let expected_validators = {
        let mut pks = GENESIS_VALIDATOR_PUBLIC_KEYS.clone();
        pks.remove(&LOWEST_STAKE_VALIDATOR);
        pks.insert(DEFAULT_ACCOUNT_PUBLIC_KEY.clone());
        pks
    };
    assert_eq!(
        next_validator_set_4, expected_validators,
        "actual next validator set does not match expected validator set"
    );
}
