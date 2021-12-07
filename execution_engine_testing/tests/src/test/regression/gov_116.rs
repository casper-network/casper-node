use std::{collections::BTreeSet, iter::FromIterator};

use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_GENESIS_TIMESTAMP_MILLIS,
    DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_VALIDATOR_SLOTS, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::core::engine_state::{genesis::GenesisValidator, GenesisAccount};
use casper_types::{
    runtime_args,
    system::{
        auction::{self, DelegationRate, EraValidators},
        mint,
    },
    Motes, PublicKey, RuntimeArgs, SecretKey, U256, U512,
};

static GENESIS_VALIDATOR_PUBLIC_KEYS: Lazy<BTreeSet<PublicKey>> = Lazy::new(|| {
    let mut vec = BTreeSet::new();
    for i in 1..=DEFAULT_VALIDATOR_SLOTS {
        let mut secret_key_bytes = [255u8; 32];
        U256::from(i).to_big_endian(&mut secret_key_bytes);
        let secret_key = SecretKey::secp256k1_from_bytes(secret_key_bytes).unwrap();
        let public_key = PublicKey::from(&secret_key);
        vec.insert(public_key);
    }
    vec
});

const MINIMUM_BONDED_AMOUNT: u64 = 1_000;

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

static GENESIS_ACCOUNTS: Lazy<Vec<GenesisAccount>> = Lazy::new(|| {
    let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
    tmp.append(&mut GENESIS_VALIDATORS.clone());
    tmp
});

fn initialize_builder() -> InMemoryWasmTestBuilder {
    let mut builder = InMemoryWasmTestBuilder::default();

    let run_genesis_request = utils::create_run_genesis_request(GENESIS_ACCOUNTS.clone());
    builder.run_genesis(&run_genesis_request);

    let fund_request_1 = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            mint::ARG_TARGET => PublicKey::System.to_account_hash(),
            mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
            mint::ARG_ID => <Option<u64>>::None,
        },
    )
    .build();

    builder.exec(fund_request_1).expect_success().commit();

    builder
}
const WITHDRAW_AMOUNT: u64 = MINIMUM_BONDED_AMOUNT - 1;

const VESTING_BASE: u64 = DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;
const WEEK_MILLIS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Simplified vesting weeks for testing purposes
const VESTING_WEEKS: [u64; 3] = [
    // Unlock genesis validator's funds after 91 days ~ 13 weeks
    VESTING_BASE + (13 * WEEK_MILLIS),
    VESTING_BASE + (14 * WEEK_MILLIS),
    VESTING_BASE + (15 * WEEK_MILLIS),
];

#[ignore]
#[test]
fn should_not_retain_genesis_validator_slot_protection_after_vesting_period_elapsed() {
    let lowest_stake_validator = {
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

        genesis_account
    };

    let unbonding_account_pk = lowest_stake_validator.public_key();
    let unbonding_account_addr = lowest_stake_validator.public_key().to_account_hash();

    let mut builder = initialize_builder();

    // Unlock all funds of genesis validator
    builder.run_auction(VESTING_WEEKS[0], Vec::new());

    let era_validators_1: EraValidators = builder.get_era_validators();

    let (last_era_1, weights_1) = era_validators_1.iter().last().unwrap();
    let genesis_validator_stake_1 = weights_1.get(&unbonding_account_pk).unwrap();
    let next_validator_set_1 = BTreeSet::from_iter(weights_1.keys().cloned());
    assert_eq!(
        next_validator_set_1,
        GENESIS_VALIDATOR_PUBLIC_KEYS.clone(),
        "expected validator set should be unchanged"
    );

    let withdraw_bid_request = {
        let auction_hash = builder.get_auction_contract_hash();
        let session_args = runtime_args! {
            auction::ARG_PUBLIC_KEY => unbonding_account_pk.clone(),
            auction::ARG_AMOUNT => U512::from(WITHDRAW_AMOUNT),
        };
        ExecuteRequestBuilder::contract_call_by_hash(
            unbonding_account_addr,
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
    let genesis_validator_stake_2 = weights_2.get(&unbonding_account_pk).unwrap();

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

    let delegation_rate: DelegationRate = 0;

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        builder.get_auction_contract_hash(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => *genesis_validator_stake_1,
            auction::ARG_DELEGATION_RATE => delegation_rate,
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
        "we're still limited to slots"
    );

    assert!(
        weights_3.contains_key(&*DEFAULT_ACCOUNT_PUBLIC_KEY),
        "new non-genesis validator should replace unbonded account"
    );

    assert!(
        !weights_3.contains_key(&unbonding_account_pk),
        "unbonded account should
        be out of the set"
    );

    let next_validator_set_3 = BTreeSet::from_iter(weights_3.keys().cloned());
    let expected_validators = {
        let mut pks = GENESIS_VALIDATOR_PUBLIC_KEYS.clone();
        pks.remove(&unbonding_account_pk);
        pks.insert(DEFAULT_ACCOUNT_PUBLIC_KEY.clone());
        pks
    };
    assert_eq!(
        next_validator_set_3, expected_validators,
        "actual next validator set does not match expected validator set"
    );
}
