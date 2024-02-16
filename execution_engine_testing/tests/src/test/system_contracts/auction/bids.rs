use std::{collections::BTreeSet, iter::FromIterator};

use assert_matches::assert_matches;
use num_traits::{One, Zero};
use once_cell::sync::Lazy;
use tempfile::TempDir;

use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, LmdbWasmTestBuilder, StepRequestBuilder, DEFAULT_ACCOUNTS,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_CHAINSPEC_REGISTRY,
    DEFAULT_EXEC_CONFIG, DEFAULT_GENESIS_CONFIG_HASH, DEFAULT_GENESIS_TIMESTAMP_MILLIS,
    DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_PROTOCOL_VERSION, DEFAULT_UNBONDING_DELAY,
    MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST, SYSTEM_ADDR,
    TIMESTAMP_MILLIS_INCREMENT,
};
use casper_execution_engine::{
    engine_state::{
        self, engine_config::DEFAULT_MINIMUM_DELEGATION_AMOUNT, EngineConfigBuilder, Error,
    },
    execution,
};
use casper_storage::data_access_layer::GenesisRequest;

use casper_types::{
    self,
    account::AccountHash,
    addressable_entity::EntityKindTag,
    api_error::ApiError,
    runtime_args,
    system::{
        self,
        auction::{
            self, BidsExt, DelegationRate, EraValidators, Error as AuctionError, UnbondingPurses,
            ValidatorWeights, ARG_AMOUNT, ARG_DELEGATION_RATE, ARG_DELEGATOR, ARG_NEW_VALIDATOR,
            ARG_PUBLIC_KEY, ARG_VALIDATOR, ERA_ID_KEY, INITIAL_ERA_ID,
        },
    },
    EntityAddr, EraId, GenesisAccount, GenesisConfigBuilder, GenesisValidator, Key, Motes,
    ProtocolVersion, PublicKey, SecretKey, U256, U512,
};

const ARG_TARGET: &str = "target";

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_ACTIVATE_BID: &str = "activate_bid.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";
const CONTRACT_WITHDRAW_BID: &str = "withdraw_bid.wasm";
const CONTRACT_DELEGATE: &str = "delegate.wasm";
const CONTRACT_UNDELEGATE: &str = "undelegate.wasm";
const CONTRACT_REDELEGATE: &str = "redelegate.wasm";

const TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE + 1000;

const ADD_BID_AMOUNT_1: u64 = 95_000;
const ADD_BID_AMOUNT_2: u64 = 47_500;
const ADD_BID_DELEGATION_RATE_1: DelegationRate = 10;
const BID_AMOUNT_2: u64 = 5_000;
const ADD_BID_DELEGATION_RATE_2: DelegationRate = 15;
const WITHDRAW_BID_AMOUNT_2: u64 = 15_000;

const DELEGATE_AMOUNT_1: u64 = 125_000 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const DELEGATE_AMOUNT_2: u64 = 15_000 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const UNDELEGATE_AMOUNT_1: u64 = 35_000;

const SYSTEM_TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;

const WEEK_MILLIS: u64 = 7 * 24 * 60 * 60 * 1000;

static NON_FOUNDER_VALIDATOR_1_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([3; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static NON_FOUNDER_VALIDATOR_1_ADDR: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*NON_FOUNDER_VALIDATOR_1_PK));

static NON_FOUNDER_VALIDATOR_2_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([4; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static NON_FOUNDER_VALIDATOR_2_ADDR: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*NON_FOUNDER_VALIDATOR_2_PK));

static ACCOUNT_1_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([200; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_1_PK));
const ACCOUNT_1_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_1_BOND: u64 = 100_000;

static ACCOUNT_2_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([202; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static ACCOUNT_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_2_PK));
const ACCOUNT_2_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_2_BOND: u64 = 200_000;

static BID_ACCOUNT_1_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([204; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static BID_ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*BID_ACCOUNT_1_PK));
const BID_ACCOUNT_1_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;

static BID_ACCOUNT_2_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([206; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static BID_ACCOUNT_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*BID_ACCOUNT_2_PK));
const BID_ACCOUNT_2_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;

static VALIDATOR_1: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([3; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_1: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([205; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_2: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([207; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALIDATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALIDATOR_1));
static DELEGATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_1));
static DELEGATOR_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_2));
const VALIDATOR_1_STAKE: u64 = 1_000_000;
const DELEGATOR_1_STAKE: u64 = 1_500_000 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const DELEGATOR_1_BALANCE: u64 = DEFAULT_ACCOUNT_INITIAL_BALANCE;
const DELEGATOR_2_STAKE: u64 = 2_000_000 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const DELEGATOR_2_BALANCE: u64 = DEFAULT_ACCOUNT_INITIAL_BALANCE;

const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 0;

const EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS: u64 =
    DEFAULT_GENESIS_TIMESTAMP_MILLIS + CASPER_LOCKED_FUNDS_PERIOD_MILLIS;

const WEEK_TIMESTAMPS: [u64; 14] = [
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS,
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + WEEK_MILLIS,
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + (WEEK_MILLIS * 2),
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + (WEEK_MILLIS * 3),
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + (WEEK_MILLIS * 4),
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + (WEEK_MILLIS * 5),
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + (WEEK_MILLIS * 6),
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + (WEEK_MILLIS * 7),
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + (WEEK_MILLIS * 8),
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + (WEEK_MILLIS * 9),
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + (WEEK_MILLIS * 10),
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + (WEEK_MILLIS * 11),
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + (WEEK_MILLIS * 12),
    EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS + (WEEK_MILLIS * 13),
];

const DAY_MILLIS: u64 = 24 * 60 * 60 * 1000;
const CASPER_VESTING_SCHEDULE_PERIOD_MILLIS: u64 = 91 * DAY_MILLIS;
const CASPER_LOCKED_FUNDS_PERIOD_MILLIS: u64 = 90 * DAY_MILLIS;

#[ignore]
#[test]
fn should_add_new_bid() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE.into()),
            None,
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    let bids = builder.get_bids();

    assert_eq!(bids.len(), 1);
    let active_bid = bids.validator_bid(&BID_ACCOUNT_1_PK.clone()).unwrap();
    assert_eq!(
        builder.get_purse_balance(*active_bid.bonding_purse()),
        U512::from(ADD_BID_AMOUNT_1)
    );
    assert_eq!(*active_bid.delegation_rate(), ADD_BID_DELEGATION_RATE_1);
}

#[ignore]
#[test]
fn should_increase_existing_bid() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE.into()),
            None,
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    // 2nd bid top-up
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK.clone(),
            ARG_AMOUNT => U512::from(BID_AMOUNT_2),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_2,
        },
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();

    let bids = builder.get_bids();

    assert_eq!(bids.len(), 1);

    let active_bid = bids.validator_bid(&BID_ACCOUNT_1_PK.clone()).unwrap();
    assert_eq!(
        builder.get_purse_balance(*active_bid.bonding_purse()),
        U512::from(ADD_BID_AMOUNT_1 + BID_AMOUNT_2)
    );
    assert_eq!(*active_bid.delegation_rate(), ADD_BID_DELEGATION_RATE_2);
}

#[ignore]
#[test]
fn should_decrease_existing_bid() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE.into()),
            None,
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let bid_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();
    builder.exec(bid_request).expect_success().commit();

    // withdraw some amount
    let withdraw_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK.clone(),
            ARG_AMOUNT => U512::from(WITHDRAW_BID_AMOUNT_2),
        },
    )
    .build();
    builder.exec(withdraw_request).commit().expect_success();

    let bids = builder.get_bids();

    assert_eq!(bids.len(), 1);

    let active_bid = bids.validator_bid(&BID_ACCOUNT_1_PK.clone()).unwrap();
    assert_eq!(
        builder.get_purse_balance(*active_bid.bonding_purse()),
        // Since we don't pay out immediately `WITHDRAW_BID_AMOUNT_2` is locked in unbonding queue
        U512::from(ADD_BID_AMOUNT_1)
    );
    let unbonding_purses: UnbondingPurses = builder.get_unbonds();
    let unbond_list = unbonding_purses
        .get(&BID_ACCOUNT_1_ADDR)
        .expect("should have unbonded");
    assert_eq!(unbond_list.len(), 1);
    let unbonding_purse = unbond_list[0].clone();
    assert_eq!(unbonding_purse.unbonder_public_key(), &*BID_ACCOUNT_1_PK);
    assert_eq!(unbonding_purse.validator_public_key(), &*BID_ACCOUNT_1_PK);

    // `WITHDRAW_BID_AMOUNT_2` is in unbonding list
    assert_eq!(unbonding_purse.amount(), &U512::from(WITHDRAW_BID_AMOUNT_2),);
    assert_eq!(unbonding_purse.era_of_creation(), INITIAL_ERA_ID,);
}

#[ignore]
#[test]
fn should_run_delegate_and_undelegate() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE.into()),
            None,
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let transfer_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    // non-founding validator request
    let add_bid_request_1 = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(transfer_request_1).expect_success().commit();
    builder.exec(transfer_request_2).expect_success().commit();
    builder.exec(add_bid_request_1).expect_success().commit();

    let auction_hash = builder.get_auction_contract_hash();

    let bids = builder.get_bids();
    assert_eq!(bids.len(), 1);
    let active_bid = bids.validator_bid(&NON_FOUNDER_VALIDATOR_1_PK).unwrap();
    assert_eq!(
        builder.get_purse_balance(*active_bid.bonding_purse()),
        U512::from(ADD_BID_AMOUNT_1)
    );
    assert_eq!(*active_bid.delegation_rate(), ADD_BID_DELEGATION_RATE_1);

    let auction_key = Key::addressable_entity_key(EntityKindTag::System, auction_hash);

    let auction_stored_value = builder
        .query(None, auction_key, &[])
        .expect("should query auction hash");
    let _auction = auction_stored_value
        .as_addressable_entity()
        .expect("should be contract");

    //
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    builder.exec(exec_request_1).commit().expect_success();

    let bids = builder.get_bids();
    assert_eq!(bids.len(), 2);
    let delegators = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK)
        .expect("should have delegators");
    assert_eq!(delegators.len(), 1);
    let delegator = bids
        .delegator_by_public_keys(&NON_FOUNDER_VALIDATOR_1_PK, &BID_ACCOUNT_1_PK)
        .expect("should have account1 delegation");
    let delegated_amount_1 = delegator.staked_amount();
    assert_eq!(delegated_amount_1, U512::from(DELEGATE_AMOUNT_1));

    // 2nd bid top-up
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_2),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    builder.exec(exec_request_2).commit().expect_success();

    let bids = builder.get_bids();
    assert_eq!(bids.len(), 2);
    let delegators = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK)
        .expect("should have delegators");
    assert_eq!(delegators.len(), 1);
    let delegator = bids
        .delegator_by_public_keys(&NON_FOUNDER_VALIDATOR_1_PK, &BID_ACCOUNT_1_PK)
        .expect("should have account1 delegation");
    let delegated_amount_1 = delegator.staked_amount();
    assert_eq!(
        delegated_amount_1,
        U512::from(DELEGATE_AMOUNT_1 + DELEGATE_AMOUNT_2)
    );

    let exec_request_3 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();
    builder.exec(exec_request_3).expect_success().commit();

    let bids = builder.get_bids();
    assert_eq!(bids.len(), 2);
    let delegators = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK)
        .expect("should have delegators");
    assert_eq!(delegators.len(), 1);
    let delegator = bids
        .delegator_by_public_keys(&NON_FOUNDER_VALIDATOR_1_PK, &BID_ACCOUNT_1_PK)
        .expect("should have account1 delegation");
    let delegated_amount_1 = delegator.staked_amount();
    assert_eq!(
        delegated_amount_1,
        U512::from(DELEGATE_AMOUNT_1 + DELEGATE_AMOUNT_2 - UNDELEGATE_AMOUNT_1)
    );

    let unbonding_purses: UnbondingPurses = builder.get_unbonds();
    assert_eq!(unbonding_purses.len(), 1);

    let unbond_list = unbonding_purses
        .get(&BID_ACCOUNT_1_ADDR)
        .expect("should have unbonding purse for non founder validator");
    assert_eq!(unbond_list.len(), 1);
    assert_eq!(
        unbond_list[0].validator_public_key(),
        &*NON_FOUNDER_VALIDATOR_1_PK
    );
    assert_eq!(unbond_list[0].unbonder_public_key(), &*BID_ACCOUNT_1_PK);
    assert_eq!(unbond_list[0].amount(), &U512::from(UNDELEGATE_AMOUNT_1));
    assert!(!unbond_list[0].is_validator());

    assert_eq!(unbond_list[0].era_of_creation(), INITIAL_ERA_ID);
}

#[ignore]
#[test]
fn should_calculate_era_validators() {
    assert_ne!(*ACCOUNT_1_ADDR, *ACCOUNT_2_ADDR,);
    assert_ne!(*ACCOUNT_2_ADDR, *BID_ACCOUNT_1_ADDR,);
    assert_ne!(*ACCOUNT_2_ADDR, *DEFAULT_ACCOUNT_ADDR,);
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let account_3 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE.into()),
            None,
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp.push(account_3);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();
    let transfer_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let auction_hash = builder.get_auction_contract_hash();
    let bids = builder.get_bids();
    assert_eq!(bids.len(), 2, "founding validators {:?}", bids);

    // Verify first era validators
    let first_validator_weights: ValidatorWeights = builder
        .get_validator_weights(INITIAL_ERA_ID)
        .expect("should have first era validator weights");
    assert_eq!(
        first_validator_weights
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from_iter(vec![ACCOUNT_1_PK.clone(), ACCOUNT_2_PK.clone()])
    );

    builder.exec(transfer_request_1).commit().expect_success();
    builder.exec(transfer_request_2).commit().expect_success();

    // non-founding validator request
    let add_bid_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(add_bid_request_1).commit().expect_success();

    let pre_era_id: EraId = builder.get_value(EntityAddr::System(auction_hash.value()), ERA_ID_KEY);
    assert_eq!(pre_era_id, EraId::from(0));

    builder.run_auction(
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
        Vec::new(),
    );

    let post_era_id: EraId =
        builder.get_value(EntityAddr::System(auction_hash.value()), ERA_ID_KEY);
    assert_eq!(post_era_id, EraId::from(1));

    let era_validators: EraValidators = builder.get_era_validators();

    // Check if there are no missing eras after the calculation, but we don't care about what the
    // elements are
    let auction_delay = builder.get_auction_delay();
    let eras: Vec<_> = era_validators.keys().copied().collect();
    assert!(!era_validators.is_empty());
    assert!(era_validators.len() >= auction_delay as usize); // definitely more than 1 element
    let (first_era, _) = era_validators.iter().min().unwrap();
    let (last_era, _) = era_validators.iter().max().unwrap();
    let expected_eras: Vec<EraId> = {
        let lo: u64 = (*first_era).into();
        let hi: u64 = (*last_era).into();
        (lo..=hi).map(EraId::from).collect()
    };
    assert_eq!(eras, expected_eras, "Eras {:?}", eras);

    assert!(post_era_id > EraId::from(0));
    let consensus_next_era_id: EraId = post_era_id + auction_delay + 1;

    let snapshot_size = auction_delay as usize + 1;
    assert_eq!(
        era_validators.len(),
        snapshot_size,
        "era_id={} {:?}",
        consensus_next_era_id,
        era_validators
    ); // eraindex==1 - ran once

    let lookup_era_id = consensus_next_era_id - 1;

    let validator_weights = era_validators
        .get(&lookup_era_id) // indexed from 0
        .unwrap_or_else(|| {
            panic!(
                "should have era_index=={} entry {:?}",
                consensus_next_era_id, era_validators
            )
        });
    assert_eq!(
        validator_weights.len(),
        3,
        "{:?} {:?}",
        era_validators,
        validator_weights
    ); //2 genesis validators "winners"
    assert_eq!(
        validator_weights
            .get(&BID_ACCOUNT_1_PK)
            .expect("should have bid account in this era"),
        &U512::from(ADD_BID_AMOUNT_1)
    );

    // Check validator weights using the API
    let era_validators_result = builder
        .get_validator_weights(lookup_era_id)
        .expect("should have validator weights");
    assert_eq!(era_validators_result, *validator_weights);

    // Make sure looked up era validators are different than initial era validators
    assert_ne!(era_validators_result, first_validator_weights);
}

#[ignore]
#[test]
fn should_get_first_seigniorage_recipients() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp
    };

    // We can't use `utils::create_run_genesis_request` as the snapshot used an auction delay of 3.
    let auction_delay = 3;
    let exec_config = GenesisConfigBuilder::new()
        .with_accounts(accounts)
        .with_auction_delay(auction_delay)
        .with_locked_funds_period_millis(CASPER_LOCKED_FUNDS_PERIOD_MILLIS)
        .build();
    let run_genesis_request = GenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        exec_config,
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    );

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let bids = builder.get_bids();
    assert_eq!(bids.len(), 2);

    let founding_validator_1 = bids
        .validator_bid(&ACCOUNT_1_PK)
        .expect("should have account 1 pk");
    assert_eq!(
        founding_validator_1
            .vesting_schedule()
            .map(|vesting_schedule| vesting_schedule.initial_release_timestamp_millis()),
        Some(DEFAULT_GENESIS_TIMESTAMP_MILLIS + CASPER_LOCKED_FUNDS_PERIOD_MILLIS)
    );

    let founding_validator_2 = bids
        .validator_bid(&ACCOUNT_2_PK)
        .expect("should have account 2 pk");
    assert_eq!(
        founding_validator_2
            .vesting_schedule()
            .map(|vesting_schedule| vesting_schedule.initial_release_timestamp_millis()),
        Some(DEFAULT_GENESIS_TIMESTAMP_MILLIS + CASPER_LOCKED_FUNDS_PERIOD_MILLIS)
    );

    builder.exec(transfer_request_1).commit().expect_success();

    // run_auction should be executed first
    builder.run_auction(
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + CASPER_LOCKED_FUNDS_PERIOD_MILLIS,
        Vec::new(),
    );

    let mut era_validators: EraValidators = builder.get_era_validators();
    let auction_delay = builder.get_auction_delay();
    let snapshot_size = auction_delay as usize + 1;

    assert_eq!(era_validators.len(), snapshot_size, "{:?}", era_validators); // eraindex==1 - ran once

    assert!(era_validators.contains_key(&(EraId::from(auction_delay).successor())));

    let era_id = EraId::from(auction_delay);

    let validator_weights = era_validators.remove(&era_id).unwrap_or_else(|| {
        panic!(
            "should have era_index=={} entry {:?}",
            era_id, era_validators
        )
    });
    // 2 genesis validators "winners" with non-zero bond
    assert_eq!(validator_weights.len(), 2, "{:?}", validator_weights);
    assert_eq!(
        validator_weights.get(&ACCOUNT_1_PK).unwrap(),
        &U512::from(ACCOUNT_1_BOND)
    );
    assert_eq!(
        validator_weights.get(&ACCOUNT_2_PK).unwrap(),
        &U512::from(ACCOUNT_2_BOND)
    );

    let first_validator_weights = builder
        .get_validator_weights(era_id)
        .expect("should have validator weights");
    assert_eq!(first_validator_weights, validator_weights);
}

#[ignore]
#[test]
fn should_release_founder_stake() {
    const NEW_MINIMUM_DELEGATION_AMOUNT: u64 = 0;

    // ACCOUNT_1_BOND / 14 = 7_142
    const EXPECTED_WEEKLY_RELEASE: u64 = 7_142;

    const EXPECTED_REMAINDER: u64 = 12;

    const EXPECTED_LOCKED_AMOUNTS: [u64; 14] = [
        92858, 85716, 78574, 71432, 64290, 57148, 50006, 42864, 35722, 28580, 21438, 14296, 7154, 0,
    ];

    let expected_locked_amounts: Vec<U512> = EXPECTED_LOCKED_AMOUNTS
        .iter()
        .cloned()
        .map(U512::from)
        .collect();

    let expect_unbond_success = |builder: &mut LmdbWasmTestBuilder, amount: u64| {
        let partial_unbond = ExecuteRequestBuilder::standard(
            *ACCOUNT_1_ADDR,
            CONTRACT_WITHDRAW_BID,
            runtime_args! {
                ARG_PUBLIC_KEY => ACCOUNT_1_PK.clone(),
                ARG_AMOUNT => U512::from(amount),
            },
        )
        .build();

        builder.exec(partial_unbond).commit().expect_success();
    };

    let expect_unbond_failure = |builder: &mut LmdbWasmTestBuilder, amount: u64| {
        let full_unbond = ExecuteRequestBuilder::standard(
            *ACCOUNT_1_ADDR,
            CONTRACT_WITHDRAW_BID,
            runtime_args! {
                ARG_PUBLIC_KEY => ACCOUNT_1_PK.clone(),
                ARG_AMOUNT => U512::from(amount),
            },
        )
        .build();

        builder.exec(full_unbond).commit();

        let error = {
            let response = builder
                .get_last_exec_result()
                .expect("should have last exec result");
            let exec_response = response.last().expect("should have response");
            exec_response
                .as_error()
                .cloned()
                .expect("should have error")
        };
        assert_matches!(
            error,
            engine_state::Error::Exec(execution::Error::Revert(ApiError::AuctionError(15)))
        );
    };

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        tmp.push(account_1);
        tmp
    };

    //let run_genesis_request = utils::create_run_genesis_request(accounts);
    let run_genesis_request = {
        let exec_config = GenesisConfigBuilder::default()
            .with_accounts(accounts)
            .with_locked_funds_period_millis(CASPER_LOCKED_FUNDS_PERIOD_MILLIS)
            .build();

        GenesisRequest::new(
            *DEFAULT_GENESIS_CONFIG_HASH,
            *DEFAULT_PROTOCOL_VERSION,
            exec_config,
            DEFAULT_CHAINSPEC_REGISTRY.clone(),
        )
    };

    let custom_engine_config = EngineConfigBuilder::default()
        .with_minimum_delegation_amount(NEW_MINIMUM_DELEGATION_AMOUNT)
        .with_vesting_schedule_period_millis(CASPER_VESTING_SCHEDULE_PERIOD_MILLIS)
        .build();

    let mut builder = LmdbWasmTestBuilder::new_temporary_with_config(custom_engine_config);

    builder.run_genesis(run_genesis_request);

    let fund_system_account = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE / 10)
        },
    )
    .build();

    builder.exec(fund_system_account).commit().expect_success();

    // Check bid and its vesting schedule
    {
        let bids = builder.get_bids();
        assert_eq!(bids.len(), 1);

        let entry = bids.validator_bid(&ACCOUNT_1_PK).unwrap();
        let vesting_schedule = entry.vesting_schedule().unwrap();

        let initial_release = vesting_schedule.initial_release_timestamp_millis();
        assert_eq!(initial_release, EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS);

        let locked_amounts = vesting_schedule.locked_amounts().map(|arr| arr.to_vec());
        assert!(locked_amounts.is_none());
    }

    builder.run_auction(DEFAULT_GENESIS_TIMESTAMP_MILLIS, Vec::new());

    {
        // Attempt unbond of one mote
        expect_unbond_failure(&mut builder, u64::one());
    }

    builder.run_auction(WEEK_TIMESTAMPS[0], Vec::new());

    // Check bid and its vesting schedule
    {
        let bids = builder.get_bids();
        assert_eq!(bids.len(), 1);

        let entry = bids.validator_bid(&ACCOUNT_1_PK).unwrap();
        let vesting_schedule = entry.vesting_schedule().unwrap();

        let initial_release = vesting_schedule.initial_release_timestamp_millis();
        assert_eq!(initial_release, EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS);

        let locked_amounts = vesting_schedule.locked_amounts().map(|arr| arr.to_vec());
        assert_eq!(locked_amounts, Some(expected_locked_amounts));
    }

    let mut total_unbonded = 0;

    {
        // Attempt full unbond
        expect_unbond_failure(&mut builder, ACCOUNT_1_BOND);

        // Attempt unbond of released amount
        expect_unbond_success(&mut builder, EXPECTED_WEEKLY_RELEASE);

        total_unbonded += EXPECTED_WEEKLY_RELEASE;

        assert_eq!(ACCOUNT_1_BOND - total_unbonded, EXPECTED_LOCKED_AMOUNTS[0])
    }

    for i in 1..13 {
        // Run auction forward by almost a week
        builder.run_auction(WEEK_TIMESTAMPS[i] - 1, Vec::new());

        // Attempt unbond of 1 mote
        expect_unbond_failure(&mut builder, u64::one());

        // Run auction forward by one millisecond
        builder.run_auction(WEEK_TIMESTAMPS[i], Vec::new());

        // Attempt unbond of more than weekly release
        expect_unbond_failure(&mut builder, EXPECTED_WEEKLY_RELEASE + 1);

        // Attempt unbond of released amount
        expect_unbond_success(&mut builder, EXPECTED_WEEKLY_RELEASE);

        total_unbonded += EXPECTED_WEEKLY_RELEASE;

        assert_eq!(ACCOUNT_1_BOND - total_unbonded, EXPECTED_LOCKED_AMOUNTS[i])
    }

    {
        // Run auction forward by almost a week
        builder.run_auction(WEEK_TIMESTAMPS[13] - 1, Vec::new());

        // Attempt unbond of 1 mote
        expect_unbond_failure(&mut builder, u64::one());

        // Run auction forward by one millisecond
        builder.run_auction(WEEK_TIMESTAMPS[13], Vec::new());

        // Attempt unbond of released amount + remainder
        expect_unbond_success(&mut builder, EXPECTED_WEEKLY_RELEASE + EXPECTED_REMAINDER);

        total_unbonded += EXPECTED_WEEKLY_RELEASE + EXPECTED_REMAINDER;

        assert_eq!(ACCOUNT_1_BOND - total_unbonded, EXPECTED_LOCKED_AMOUNTS[13])
    }

    assert_eq!(ACCOUNT_1_BOND, total_unbonded);
}

#[ignore]
#[test]
fn should_fail_to_get_era_validators() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    assert_eq!(
        builder.get_validator_weights(EraId::MAX),
        None,
        "should not have era validators for invalid era"
    );
}

#[ignore]
#[test]
fn should_use_era_validators_endpoint_for_first_era() {
    let extra_accounts = vec![GenesisAccount::account(
        ACCOUNT_1_PK.clone(),
        Motes::new(ACCOUNT_1_BALANCE.into()),
        Some(GenesisValidator::new(
            Motes::new(ACCOUNT_1_BOND.into()),
            DelegationRate::zero(),
        )),
    )];

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.extend(extra_accounts);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let validator_weights = builder
        .get_validator_weights(INITIAL_ERA_ID)
        .expect("should have validator weights for era 0");

    assert_eq!(validator_weights.len(), 1);
    assert_eq!(validator_weights[&ACCOUNT_1_PK], ACCOUNT_1_BOND.into());

    let era_validators: EraValidators = builder.get_era_validators();
    assert_eq!(era_validators[&EraId::from(0)], validator_weights);
}

#[ignore]
#[test]
fn should_calculate_era_validators_multiple_new_bids() {
    assert_ne!(*ACCOUNT_1_ADDR, *ACCOUNT_2_ADDR,);
    assert_ne!(*ACCOUNT_2_ADDR, *BID_ACCOUNT_1_ADDR,);
    assert_ne!(*ACCOUNT_2_ADDR, *DEFAULT_ACCOUNT_ADDR,);
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let account_3 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE.into()),
            None,
        );
        let account_4 = GenesisAccount::account(
            BID_ACCOUNT_2_PK.clone(),
            Motes::new(BID_ACCOUNT_2_BALANCE.into()),
            None,
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp.push(account_3);
        tmp.push(account_4);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let genesis_validator_weights = builder
        .get_validator_weights(INITIAL_ERA_ID)
        .expect("should have genesis validators for initial era");
    let auction_delay = builder.get_auction_delay();
    // new_era is the first era in the future where new era validator weights will be calculated
    let new_era = INITIAL_ERA_ID + auction_delay + 1;
    assert!(builder.get_validator_weights(new_era).is_none());
    assert_eq!(
        builder.get_validator_weights(new_era - 1).unwrap(),
        builder.get_validator_weights(INITIAL_ERA_ID).unwrap()
    );

    assert_eq!(
        genesis_validator_weights
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from_iter(vec![ACCOUNT_1_PK.clone(), ACCOUNT_2_PK.clone()])
    );

    // Fund additional accounts
    for target in &[
        *SYSTEM_ADDR,
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        *NON_FOUNDER_VALIDATOR_2_ADDR,
    ] {
        let transfer_request_1 = ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CONTRACT_TRANSFER_TO_ACCOUNT,
            runtime_args! {
                ARG_TARGET => *target,
                ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
            },
        )
        .build();
        builder.exec(transfer_request_1).commit().expect_success();
    }

    // non-founding validator request
    let add_bid_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();
    let add_bid_request_2 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_2_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_2),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_2,
        },
    )
    .build();

    builder.exec(add_bid_request_1).commit().expect_success();
    builder.exec(add_bid_request_2).commit().expect_success();

    // run auction and compute validators for new era
    builder.run_auction(
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
        Vec::new(),
    );
    // Verify first era validators
    let new_validator_weights: ValidatorWeights = builder
        .get_validator_weights(new_era)
        .expect("should have first era validator weights");

    // check that the new computed era has exactly the state we expect
    let lhs = new_validator_weights
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();

    let rhs = BTreeSet::from_iter(vec![
        ACCOUNT_1_PK.clone(),
        ACCOUNT_2_PK.clone(),
        BID_ACCOUNT_1_PK.clone(),
        BID_ACCOUNT_2_PK.clone(),
    ]);

    assert_eq!(lhs, rhs);

    // make sure that new validators are exactly those that were part of add_bid requests
    let new_validators: BTreeSet<_> = rhs
        .difference(&genesis_validator_weights.keys().cloned().collect())
        .cloned()
        .collect();
    assert_eq!(
        new_validators,
        BTreeSet::from_iter(vec![BID_ACCOUNT_1_PK.clone(), BID_ACCOUNT_2_PK.clone(),])
    );
}

#[ignore]
#[test]
fn undelegated_funds_should_be_released() {
    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(SYSTEM_TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        delegator_1_fund_request,
        validator_1_fund_request,
        validator_1_add_bid_request,
        delegator_1_validator_1_delegate_request,
    ];

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let delegator_1_undelegate_purse = builder
        .get_entity_by_account_hash(*BID_ACCOUNT_1_ADDR)
        .expect("should have default account")
        .main_purse();

    let delegator_1_undelegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    builder
        .exec(delegator_1_undelegate_request)
        .commit()
        .expect_success();

    let delegator_1_purse_balance_before = builder.get_purse_balance(delegator_1_undelegate_purse);

    let unbonding_delay = builder.get_unbonding_delay();

    for _ in 0..=unbonding_delay {
        let delegator_1_undelegate_purse_balance =
            builder.get_purse_balance(delegator_1_undelegate_purse);
        assert_eq!(
            delegator_1_purse_balance_before,
            delegator_1_undelegate_purse_balance
        );

        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let delegator_1_undelegate_purse_balance =
        builder.get_purse_balance(delegator_1_undelegate_purse);
    assert_eq!(
        delegator_1_undelegate_purse_balance,
        delegator_1_purse_balance_before + U512::from(UNDELEGATE_AMOUNT_1)
    )
}

#[ignore]
#[test]
fn fully_undelegated_funds_should_be_released() {
    const SYSTEM_TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(SYSTEM_TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        delegator_1_fund_request,
        validator_1_fund_request,
        validator_1_add_bid_request,
        delegator_1_validator_1_delegate_request,
    ];

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let delegator_1_undelegate_purse = builder
        .get_entity_by_account_hash(*BID_ACCOUNT_1_ADDR)
        .expect("should have default account")
        .main_purse();

    let delegator_1_undelegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    builder
        .exec(delegator_1_undelegate_request)
        .commit()
        .expect_success();

    let delegator_1_purse_balance_before = builder.get_purse_balance(delegator_1_undelegate_purse);

    let unbonding_delay = builder.get_unbonding_delay();

    for _ in 0..=unbonding_delay {
        let delegator_1_undelegate_purse_balance =
            builder.get_purse_balance(delegator_1_undelegate_purse);
        assert_eq!(
            delegator_1_undelegate_purse_balance,
            delegator_1_purse_balance_before
        );
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let delegator_1_undelegate_purse_after =
        builder.get_purse_balance(delegator_1_undelegate_purse);

    assert_eq!(
        delegator_1_undelegate_purse_after - delegator_1_purse_balance_before,
        U512::from(DELEGATE_AMOUNT_1)
    )
}

#[ignore]
#[test]
fn should_undelegate_delegators_when_validator_unbonds() {
    const VALIDATOR_1_REMAINING_BID: u64 = 1;
    const VALIDATOR_1_WITHDRAW_AMOUNT: u64 = VALIDATOR_1_STAKE - VALIDATOR_1_REMAINING_BID;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
        },
    )
    .build();

    let validator_1_partial_withdraw_bid = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
            ARG_AMOUNT => U512::from(VALIDATOR_1_WITHDRAW_AMOUNT),
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        validator_1_add_bid_request,
        delegator_1_delegate_request,
        delegator_2_delegate_request,
        validator_1_partial_withdraw_bid,
    ];

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let bids_before = builder.get_bids();
    let validator_1_bid = bids_before
        .validator_bid(&VALIDATOR_1)
        .expect("should have validator 1 bid");
    let delegators = bids_before
        .delegators_by_validator_public_key(validator_1_bid.validator_public_key())
        .expect("should have delegators");
    let delegator_keys = delegators
        .iter()
        .map(|x| x.delegator_public_key())
        .cloned()
        .collect::<BTreeSet<PublicKey>>();
    assert_eq!(
        delegator_keys,
        BTreeSet::from_iter(vec![DELEGATOR_1.clone(), DELEGATOR_2.clone()])
    );

    // Validator partially unbonds and only one entry is present
    let unbonding_purses_before: UnbondingPurses = builder.get_unbonds();
    assert_eq!(unbonding_purses_before[&*VALIDATOR_1_ADDR].len(), 1);
    assert_eq!(
        unbonding_purses_before[&*VALIDATOR_1_ADDR][0].unbonder_public_key(),
        &*VALIDATOR_1
    );

    let validator_1_withdraw_bid = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
            ARG_AMOUNT => U512::from(VALIDATOR_1_REMAINING_BID),
        },
    )
    .build();

    builder
        .exec(validator_1_withdraw_bid)
        .commit()
        .expect_success();

    let bids_after = builder.get_bids();
    assert!(bids_after.validator_bid(&VALIDATOR_1).is_none());

    let unbonding_purses_after: UnbondingPurses = builder.get_unbonds();
    assert_ne!(unbonding_purses_after, unbonding_purses_before);

    let validator1 = unbonding_purses_after
        .get(&VALIDATOR_1_ADDR)
        .expect("should have validator1");

    let validator1_unbonding = validator1
        .iter()
        .find(|x| x.validator_public_key() == &*VALIDATOR_1)
        .expect("should have validator1 unbonding");

    assert_eq!(
        validator1_unbonding.amount(),
        &U512::from(VALIDATOR_1_WITHDRAW_AMOUNT),
        "expected validator1 amount to match"
    );

    let delegator1 = unbonding_purses_after
        .get(&DELEGATOR_1_ADDR)
        .expect("should have delegator1");

    let delegator1_unbonding = delegator1
        .iter()
        .find(|x| x.unbonder_public_key() == &*DELEGATOR_1)
        .expect("should have delegator1 unbonding");

    assert_eq!(
        delegator1_unbonding.amount(),
        &U512::from(DELEGATOR_1_STAKE),
        "expected delegator1 amount to match"
    );

    let delegator2 = unbonding_purses_after
        .get(&DELEGATOR_2_ADDR)
        .expect("should have delegator2");

    let delegator2_unbonding = delegator2
        .iter()
        .find(|x| x.unbonder_public_key() == &*DELEGATOR_2)
        .expect("should have delegator2 unbonding");

    assert_eq!(
        delegator2_unbonding.amount(),
        &U512::from(DELEGATOR_2_STAKE),
        "expected delegator2 amount to match"
    );

    // Process unbonding requests to verify delegators recevied their stakes
    let validator_1 = builder
        .get_entity_by_account_hash(*VALIDATOR_1_ADDR)
        .expect("should have validator 1 account");
    let validator_1_balance_before = builder.get_purse_balance(validator_1.main_purse());

    let delegator_1 = builder
        .get_entity_by_account_hash(*DELEGATOR_1_ADDR)
        .expect("should have delegator 1 account");
    let delegator_1_balance_before = builder.get_purse_balance(delegator_1.main_purse());

    let delegator_2 = builder
        .get_entity_by_account_hash(*DELEGATOR_2_ADDR)
        .expect("should have delegator 1 account");
    let delegator_2_balance_before = builder.get_purse_balance(delegator_2.main_purse());

    for _ in 0..=DEFAULT_UNBONDING_DELAY {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let validator_1_balance_after = builder.get_purse_balance(validator_1.main_purse());
    let delegator_1_balance_after = builder.get_purse_balance(delegator_1.main_purse());
    let delegator_2_balance_after = builder.get_purse_balance(delegator_2.main_purse());

    assert_eq!(
        validator_1_balance_before + U512::from(VALIDATOR_1_STAKE),
        validator_1_balance_after
    );
    assert_eq!(
        delegator_1_balance_before + U512::from(DELEGATOR_1_STAKE),
        delegator_1_balance_after
    );
    assert_eq!(
        delegator_2_balance_before + U512::from(DELEGATOR_2_STAKE),
        delegator_2_balance_after
    );
}

#[ignore]
#[test]
fn should_undelegate_delegators_when_validator_fully_unbonds() {
    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
        },
    )
    .build();

    let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATOR_2_STAKE),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        validator_1_add_bid_request,
        delegator_1_delegate_request,
        delegator_2_delegate_request,
    ];

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    // Fully unbond
    let validator_1_withdraw_bid = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
            ARG_AMOUNT => U512::from(VALIDATOR_1_STAKE),
        },
    )
    .build();

    builder
        .exec(validator_1_withdraw_bid)
        .commit()
        .expect_success();

    let bids_after = builder.get_bids();
    assert!(bids_after.validator_bid(&VALIDATOR_1).is_none());

    let unbonding_purses_before: UnbondingPurses = builder.get_unbonds();

    let validator_1_unbonding_purse = unbonding_purses_before
        .get(&VALIDATOR_1_ADDR)
        .expect("should have unbonding purse entry")
        .iter()
        .find(|x| x.unbonder_public_key() == &*VALIDATOR_1)
        .expect("should have unbonding purse");

    let delegator_1_unbonding_purse = unbonding_purses_before
        .get(&DELEGATOR_1_ADDR)
        .expect("should have unbonding purse entry")
        .iter()
        .find(|x| x.unbonder_public_key() == &*DELEGATOR_1)
        .expect("should have unbonding purse");

    let delegator_2_unbonding_purse = unbonding_purses_before
        .get(&DELEGATOR_2_ADDR)
        .expect("should have unbonding purse entry")
        .iter()
        .find(|x| x.unbonder_public_key() == &*DELEGATOR_2)
        .expect("should have unbonding purse");

    assert_eq!(
        validator_1_unbonding_purse.amount(),
        &U512::from(VALIDATOR_1_STAKE)
    );
    assert_eq!(
        delegator_1_unbonding_purse.amount(),
        &U512::from(DELEGATOR_1_STAKE)
    );
    assert_eq!(
        delegator_2_unbonding_purse.amount(),
        &U512::from(DELEGATOR_2_STAKE)
    );

    // Process unbonding requests to verify delegators received their stakes
    let validator_1 = builder
        .get_entity_by_account_hash(*VALIDATOR_1_ADDR)
        .expect("should have validator 1 account");
    let validator_1_balance_before = builder.get_purse_balance(validator_1.main_purse());

    let delegator_1 = builder
        .get_entity_by_account_hash(*DELEGATOR_1_ADDR)
        .expect("should have delegator 1 account");
    let delegator_1_balance_before = builder.get_purse_balance(delegator_1.main_purse());

    let delegator_2 = builder
        .get_entity_by_account_hash(*DELEGATOR_2_ADDR)
        .expect("should have delegator 1 account");
    let delegator_2_balance_before = builder.get_purse_balance(delegator_2.main_purse());

    for _ in 0..=DEFAULT_UNBONDING_DELAY {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let validator_1_balance_after = builder.get_purse_balance(validator_1.main_purse());
    let delegator_1_balance_after = builder.get_purse_balance(delegator_1.main_purse());
    let delegator_2_balance_after = builder.get_purse_balance(delegator_2.main_purse());

    assert_eq!(
        validator_1_balance_before + U512::from(VALIDATOR_1_STAKE),
        validator_1_balance_after
    );
    assert_eq!(
        delegator_1_balance_before + U512::from(DELEGATOR_1_STAKE),
        delegator_1_balance_after
    );
    assert_eq!(
        delegator_2_balance_before + U512::from(DELEGATOR_2_STAKE),
        delegator_2_balance_after
    );
}

#[ignore]
#[test]
fn should_handle_evictions() {
    let activate_bid = |builder: &mut LmdbWasmTestBuilder, validator_public_key: PublicKey| {
        const ARG_VALIDATOR_PUBLIC_KEY: &str = "validator_public_key";
        let run_request = ExecuteRequestBuilder::standard(
            AccountHash::from(&validator_public_key),
            CONTRACT_ACTIVATE_BID,
            runtime_args! {
                ARG_VALIDATOR_PUBLIC_KEY => validator_public_key,
            },
        )
        .build();
        builder.exec(run_request).expect_success().commit();
    };

    let latest_validators = |builder: &mut LmdbWasmTestBuilder| {
        let era_validators: EraValidators = builder.get_era_validators();
        let validators = era_validators
            .iter()
            .rev()
            .next()
            .map(|(_era_id, validators)| validators)
            .expect("should have validators");
        validators.keys().cloned().collect::<BTreeSet<PublicKey>>()
    };

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let account_3 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(300_000.into()),
                DelegationRate::zero(),
            )),
        );
        let account_4 = GenesisAccount::account(
            BID_ACCOUNT_2_PK.clone(),
            Motes::new(BID_ACCOUNT_2_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(400_000.into()),
                DelegationRate::zero(),
            )),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp.push(account_3);
        tmp.push(account_4);
        tmp
    };

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(SYSTEM_TRANSFER_AMOUNT)
        },
    )
    .build();

    let mut timestamp = DEFAULT_GENESIS_TIMESTAMP_MILLIS;

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    builder.exec(system_fund_request).expect_success().commit();

    // No evictions
    builder.run_auction(timestamp, Vec::new());
    timestamp += WEEK_MILLIS;

    assert_eq!(
        latest_validators(&mut builder),
        BTreeSet::from_iter(vec![
            ACCOUNT_1_PK.clone(),
            ACCOUNT_2_PK.clone(),
            BID_ACCOUNT_1_PK.clone(),
            BID_ACCOUNT_2_PK.clone()
        ])
    );

    // Evict BID_ACCOUNT_1_PK and BID_ACCOUNT_2_PK
    builder.run_auction(
        timestamp,
        vec![BID_ACCOUNT_1_PK.clone(), BID_ACCOUNT_2_PK.clone()],
    );
    timestamp += WEEK_MILLIS;

    assert_eq!(
        latest_validators(&mut builder),
        BTreeSet::from_iter(vec![ACCOUNT_1_PK.clone(), ACCOUNT_2_PK.clone(),])
    );

    // Activate BID_ACCOUNT_1_PK
    activate_bid(&mut builder, BID_ACCOUNT_1_PK.clone());
    builder.run_auction(timestamp, Vec::new());
    timestamp += WEEK_MILLIS;

    assert_eq!(
        latest_validators(&mut builder),
        BTreeSet::from_iter(vec![
            ACCOUNT_1_PK.clone(),
            ACCOUNT_2_PK.clone(),
            BID_ACCOUNT_1_PK.clone()
        ])
    );

    // Activate BID_ACCOUNT_2_PK
    activate_bid(&mut builder, BID_ACCOUNT_2_PK.clone());
    builder.run_auction(timestamp, Vec::new());
    timestamp += WEEK_MILLIS;

    assert_eq!(
        latest_validators(&mut builder),
        BTreeSet::from_iter(vec![
            ACCOUNT_1_PK.clone(),
            ACCOUNT_2_PK.clone(),
            BID_ACCOUNT_1_PK.clone(),
            BID_ACCOUNT_2_PK.clone()
        ])
    );

    // Evict all validators
    builder.run_auction(
        timestamp,
        vec![
            ACCOUNT_1_PK.clone(),
            ACCOUNT_2_PK.clone(),
            BID_ACCOUNT_1_PK.clone(),
            BID_ACCOUNT_2_PK.clone(),
        ],
    );
    timestamp += WEEK_MILLIS;

    assert_eq!(latest_validators(&mut builder), BTreeSet::new());

    // Activate all validators
    for validator in &[
        ACCOUNT_1_PK.clone(),
        ACCOUNT_2_PK.clone(),
        BID_ACCOUNT_1_PK.clone(),
        BID_ACCOUNT_2_PK.clone(),
    ] {
        activate_bid(&mut builder, validator.clone());
    }
    builder.run_auction(timestamp, Vec::new());

    assert_eq!(
        latest_validators(&mut builder),
        BTreeSet::from_iter(vec![
            ACCOUNT_1_PK.clone(),
            ACCOUNT_2_PK.clone(),
            BID_ACCOUNT_1_PK.clone(),
            BID_ACCOUNT_2_PK.clone()
        ])
    );
}

#[should_panic(expected = "OrphanedDelegator")]
#[ignore]
#[test]
fn should_validate_orphaned_genesis_delegators() {
    let missing_validator_secret_key = SecretKey::ed25519_from_bytes([123; 32]).unwrap();
    let missing_validator = PublicKey::from(&missing_validator_secret_key);

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE.into()),
            Motes::new(DELEGATOR_1_STAKE.into()),
        );
        let orphaned_delegator = GenesisAccount::delegator(
            missing_validator,
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE.into()),
            Motes::new(DELEGATOR_1_STAKE.into()),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp.push(delegator_1);
        tmp.push(orphaned_delegator);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);
}

#[should_panic(expected = "DuplicatedDelegatorEntry")]
#[ignore]
#[test]
fn should_validate_duplicated_genesis_delegators() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE.into()),
            Motes::new(DELEGATOR_1_STAKE.into()),
        );
        let duplicated_delegator_1 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE.into()),
            Motes::new(DELEGATOR_1_STAKE.into()),
        );
        let duplicated_delegator_2 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_2.clone(),
            Motes::new(DELEGATOR_2_BALANCE.into()),
            Motes::new(DELEGATOR_2_STAKE.into()),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp.push(delegator_1);
        tmp.push(duplicated_delegator_1);
        tmp.push(duplicated_delegator_2);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);
}

#[should_panic(expected = "InvalidDelegationRate")]
#[ignore]
#[test]
fn should_validate_delegation_rate_of_genesis_validator() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND.into()),
                DelegationRate::max_value(),
            )),
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);
}

#[should_panic(expected = "InvalidBondAmount")]
#[ignore]
#[test]
fn should_validate_bond_amount_of_genesis_validator() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(Motes::zero(), DelegationRate::zero())),
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);
}

#[ignore]
#[test]
fn should_setup_genesis_delegators() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(Motes::new(ACCOUNT_1_BOND.into()), 80)),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE.into()),
            Motes::new(DELEGATOR_1_STAKE.into()),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp.push(delegator_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let _account_1 = builder
        .get_entity_by_account_hash(*ACCOUNT_1_ADDR)
        .expect("should install account 1");
    let _account_2 = builder
        .get_entity_by_account_hash(*ACCOUNT_2_ADDR)
        .expect("should install account 2");

    let delegator_1 = builder
        .get_entity_by_account_hash(*DELEGATOR_1_ADDR)
        .expect("should install delegator 1");
    assert_eq!(
        builder.get_purse_balance(delegator_1.main_purse()),
        U512::from(DELEGATOR_1_BALANCE)
    );

    let bids = builder.get_bids();
    let key_map = bids.public_key_map();
    let validator_keys = key_map.keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        validator_keys,
        BTreeSet::from_iter(vec![ACCOUNT_1_PK.clone(), ACCOUNT_2_PK.clone(),])
    );

    let account_1_bid_entry = bids
        .validator_bid(&ACCOUNT_1_PK)
        .expect("should have account 1 bid");
    assert_eq!(*account_1_bid_entry.delegation_rate(), 80);
    let delegators = bids
        .delegators_by_validator_public_key(&ACCOUNT_1_PK)
        .expect("should have delegators");
    assert_eq!(delegators.len(), 1);
    let delegator = delegators.first().expect("should have delegator");
    assert_eq!(
        delegator.delegator_public_key(),
        &*DELEGATOR_1,
        "should be DELEGATOR_1"
    );
    assert_eq!(delegator.staked_amount(), U512::from(DELEGATOR_1_STAKE));
}

#[ignore]
#[test]
fn should_not_partially_undelegate_uninitialized_vesting_schedule() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let validator_1 = GenesisAccount::account(
            VALIDATOR_1.clone(),
            Motes::new(VALIDATOR_1_STAKE.into()),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE.into()),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            VALIDATOR_1.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Motes::new(DELEGATOR_1_STAKE.into()),
        );
        tmp.push(validator_1);
        tmp.push(delegator_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let fund_delegator_account = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
        },
    )
    .build();
    builder
        .exec(fund_delegator_account)
        .commit()
        .expect_success();

    let partial_undelegate = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            auction::ARG_VALIDATOR => VALIDATOR_1.clone(),
            auction::ARG_DELEGATOR => DELEGATOR_1.clone(),
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE - 1),
        },
    )
    .build();

    builder.exec(partial_undelegate).commit();
    let error = {
        let response = builder
            .get_last_exec_result()
            .expect("should have last exec result");
        let exec_response = response.last().expect("should have response");
        exec_response
            .as_error()
            .cloned()
            .expect("should have error")
    };

    assert!(matches!(
        error,
        engine_state::Error::Exec(execution::Error::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == system::auction::Error::DelegatorFundsLocked as u8
    ));
}

#[ignore]
#[test]
fn should_not_fully_undelegate_uninitialized_vesting_schedule() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let validator_1 = GenesisAccount::account(
            VALIDATOR_1.clone(),
            Motes::new(VALIDATOR_1_STAKE.into()),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE.into()),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            VALIDATOR_1.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Motes::new(DELEGATOR_1_STAKE.into()),
        );
        tmp.push(validator_1);
        tmp.push(delegator_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let fund_delegator_account = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
        },
    )
    .build();
    builder
        .exec(fund_delegator_account)
        .commit()
        .expect_success();

    let full_undelegate = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            auction::ARG_VALIDATOR => VALIDATOR_1.clone(),
            auction::ARG_DELEGATOR => DELEGATOR_1.clone(),
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
        },
    )
    .build();

    builder.exec(full_undelegate).commit();
    let error = {
        let response = builder
            .get_last_exec_result()
            .expect("should have last exec result");
        let exec_response = response.last().expect("should have response");
        exec_response
            .as_error()
            .cloned()
            .expect("should have error")
    };

    assert!(matches!(
        error,
        engine_state::Error::Exec(execution::Error::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == system::auction::Error::DelegatorFundsLocked as u8
    ));
}

#[ignore]
#[test]
fn should_not_undelegate_vfta_holder_stake() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let validator_1 = GenesisAccount::account(
            VALIDATOR_1.clone(),
            Motes::new(VALIDATOR_1_STAKE.into()),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE.into()),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            VALIDATOR_1.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Motes::new(DELEGATOR_1_STAKE.into()),
        );
        tmp.push(validator_1);
        tmp.push(delegator_1);
        tmp
    };

    let run_genesis_request = {
        let exec_config = GenesisConfigBuilder::default()
            .with_accounts(accounts)
            .with_locked_funds_period_millis(CASPER_LOCKED_FUNDS_PERIOD_MILLIS)
            .build();

        GenesisRequest::new(
            *DEFAULT_GENESIS_CONFIG_HASH,
            *DEFAULT_PROTOCOL_VERSION,
            exec_config,
            DEFAULT_CHAINSPEC_REGISTRY.clone(),
        )
    };
    let custom_engine_config = EngineConfigBuilder::default()
        .with_vesting_schedule_period_millis(CASPER_VESTING_SCHEDULE_PERIOD_MILLIS)
        .build();

    let mut builder = LmdbWasmTestBuilder::new_temporary_with_config(custom_engine_config);

    builder.run_genesis(run_genesis_request);

    let post_genesis_requests = {
        let fund_delegator_account = ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CONTRACT_TRANSFER_TO_ACCOUNT,
            runtime_args! {
                ARG_TARGET => *DELEGATOR_1_ADDR,
                ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
            },
        )
        .build();

        let fund_system_account = ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CONTRACT_TRANSFER_TO_ACCOUNT,
            runtime_args! {
                ARG_TARGET => *SYSTEM_ADDR,
                ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
            },
        )
        .build();

        vec![fund_system_account, fund_delegator_account]
    };

    for post_genesis_request in post_genesis_requests {
        builder.exec(post_genesis_request).commit().expect_success();
    }

    {
        let bids = builder.get_bids();
        let delegator = bids
            .delegator_by_public_keys(&VALIDATOR_1, &DELEGATOR_1)
            .expect("should have delegator");
        let vesting_schedule = delegator
            .vesting_schedule()
            .expect("should have delegator vesting schedule");
        assert!(
            vesting_schedule.locked_amounts().is_none(),
            "should not be locked"
        );
    }

    builder.run_auction(WEEK_TIMESTAMPS[0], Vec::new());

    {
        let bids = builder.get_bids();
        let delegator = bids
            .delegator_by_public_keys(&VALIDATOR_1, &DELEGATOR_1)
            .expect("should have delegator");
        let vesting_schedule = delegator
            .vesting_schedule()
            .expect("should have vesting schedule");
        assert!(
            vesting_schedule.locked_amounts().is_some(),
            "should be locked"
        );
    }

    let partial_unbond = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            auction::ARG_VALIDATOR => VALIDATOR_1.clone(),
            auction::ARG_DELEGATOR => DELEGATOR_1.clone(),
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE - 1),
        },
    )
    .build();
    builder.exec(partial_unbond).commit();
    let error = {
        let response = builder
            .get_last_exec_result()
            .expect("should have last exec result");
        let exec_response = response.last().expect("should have response");
        exec_response
            .as_error()
            .cloned()
            .expect("should have error")
    };

    assert!(matches!(
        error,
        engine_state::Error::Exec(execution::Error::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == system::auction::Error::DelegatorFundsLocked as u8
    ));
}

#[ignore]
#[test]
fn should_release_vfta_holder_stake() {
    const EXPECTED_WEEKLY_RELEASE: u64 =
        (DELEGATOR_1_STAKE - DEFAULT_MINIMUM_DELEGATION_AMOUNT) / 14;
    const DELEGATOR_VFTA_STAKE: u64 = DELEGATOR_1_STAKE - DEFAULT_MINIMUM_DELEGATION_AMOUNT;
    const EXPECTED_REMAINDER: u64 = 12;
    const NEW_MINIMUM_DELEGATION_AMOUNT: u64 = 0;
    const EXPECTED_LOCKED_AMOUNTS: [u64; 14] = [
        1392858, 1285716, 1178574, 1071432, 964290, 857148, 750006, 642864, 535722, 428580, 321438,
        214296, 107154, 0,
    ];

    let expected_locked_amounts: Vec<U512> = EXPECTED_LOCKED_AMOUNTS
        .iter()
        .cloned()
        .map(U512::from)
        .collect();

    let expect_undelegate_success = |builder: &mut LmdbWasmTestBuilder, amount: u64| {
        let partial_unbond = ExecuteRequestBuilder::standard(
            *DELEGATOR_1_ADDR,
            CONTRACT_UNDELEGATE,
            runtime_args! {
                auction::ARG_VALIDATOR => ACCOUNT_1_PK.clone(),
                auction::ARG_DELEGATOR => DELEGATOR_1.clone(),
                ARG_AMOUNT => U512::from(amount),
            },
        )
        .build();

        builder.exec(partial_unbond).commit().expect_success();
    };

    let expect_undelegate_failure = |builder: &mut LmdbWasmTestBuilder, amount: u64| {
        let full_undelegate = ExecuteRequestBuilder::standard(
            *DELEGATOR_1_ADDR,
            CONTRACT_UNDELEGATE,
            runtime_args! {
                auction::ARG_VALIDATOR => ACCOUNT_1_PK.clone(),
                auction::ARG_DELEGATOR => DELEGATOR_1.clone(),
                ARG_AMOUNT => U512::from(amount),
            },
        )
        .build();

        builder.exec(full_undelegate).commit();

        let error = {
            let response = builder
                .get_last_exec_result()
                .expect("should have last exec result");
            let exec_response = response.last().expect("should have response");
            exec_response
                .as_error()
                .cloned()
                .expect("should have error")
        };

        assert!(
            matches!(
                error,
                engine_state::Error::Exec(execution::Error::Revert(ApiError::AuctionError(auction_error)))
                if auction_error == system::auction::Error::DelegatorFundsLocked as u8
            ),
            "{:?}",
            error
        );
    };

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE.into()),
            Motes::new(DELEGATOR_VFTA_STAKE.into()),
        );
        tmp.push(account_1);
        tmp.push(delegator_1);
        tmp
    };

    let run_genesis_request = {
        let exec_config = GenesisConfigBuilder::default()
            .with_accounts(accounts)
            .with_locked_funds_period_millis(CASPER_LOCKED_FUNDS_PERIOD_MILLIS)
            .build();

        GenesisRequest::new(
            *DEFAULT_GENESIS_CONFIG_HASH,
            *DEFAULT_PROTOCOL_VERSION,
            exec_config,
            DEFAULT_CHAINSPEC_REGISTRY.clone(),
        )
    };

    let custom_engine_config = EngineConfigBuilder::default()
        .with_minimum_delegation_amount(NEW_MINIMUM_DELEGATION_AMOUNT)
        .with_vesting_schedule_period_millis(CASPER_VESTING_SCHEDULE_PERIOD_MILLIS)
        .build();

    let mut builder = LmdbWasmTestBuilder::new_temporary_with_config(custom_engine_config);

    builder.run_genesis(run_genesis_request);

    let fund_delegator_account = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
        },
    )
    .build();
    builder
        .exec(fund_delegator_account)
        .commit()
        .expect_success();

    let fund_system_account = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
        },
    )
    .build();

    builder.exec(fund_system_account).commit().expect_success();

    // Check bid and its vesting schedule
    {
        let bids = builder.get_bids();
        assert_eq!(bids.len(), 2);
        let delegator = bids
            .delegator_by_public_keys(&ACCOUNT_1_PK, &DELEGATOR_1)
            .expect("should have delegator");

        let vesting_schedule = delegator
            .vesting_schedule()
            .expect("should have delegator vesting schedule");

        let initial_release = vesting_schedule.initial_release_timestamp_millis();
        assert_eq!(initial_release, EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS);

        let locked_amounts = vesting_schedule.locked_amounts().map(|arr| arr.to_vec());
        assert!(locked_amounts.is_none());
    }

    builder.run_auction(DEFAULT_GENESIS_TIMESTAMP_MILLIS, Vec::new());

    {
        // Attempt unbond of one mote
        expect_undelegate_failure(&mut builder, u64::one());
    }

    builder.run_auction(WEEK_TIMESTAMPS[0], Vec::new());

    // Check bid and its vesting schedule
    {
        let bids = builder.get_bids();
        assert_eq!(bids.len(), 2);
        let delegator = bids
            .delegator_by_public_keys(&ACCOUNT_1_PK, &DELEGATOR_1)
            .expect("should have delegator");

        let vesting_schedule = delegator
            .vesting_schedule()
            .expect("should have delegator vesting schedule");

        let initial_release = vesting_schedule.initial_release_timestamp_millis();
        assert_eq!(initial_release, EXPECTED_INITIAL_RELEASE_TIMESTAMP_MILLIS);

        let locked_amounts = vesting_schedule.locked_amounts().map(|arr| arr.to_vec());
        assert_eq!(locked_amounts, Some(expected_locked_amounts));
    }

    let mut total_unbonded = 0;

    {
        // Attempt full unbond
        expect_undelegate_failure(&mut builder, DELEGATOR_VFTA_STAKE);

        // Attempt unbond of released amount
        expect_undelegate_success(&mut builder, EXPECTED_WEEKLY_RELEASE);

        total_unbonded += EXPECTED_WEEKLY_RELEASE;

        assert_eq!(
            DELEGATOR_VFTA_STAKE - total_unbonded,
            EXPECTED_LOCKED_AMOUNTS[0]
        )
    }

    for i in 1..13 {
        // Run auction forward by almost a week
        builder.run_auction(WEEK_TIMESTAMPS[i] - 1, Vec::new());

        // Attempt unbond of 1 mote
        expect_undelegate_failure(&mut builder, u64::one());

        // Run auction forward by one millisecond
        builder.run_auction(WEEK_TIMESTAMPS[i], Vec::new());

        // Attempt unbond of more than weekly release
        expect_undelegate_failure(&mut builder, EXPECTED_WEEKLY_RELEASE + 1);

        // Attempt unbond of released amount
        expect_undelegate_success(&mut builder, EXPECTED_WEEKLY_RELEASE);

        total_unbonded += EXPECTED_WEEKLY_RELEASE;

        assert_eq!(
            DELEGATOR_VFTA_STAKE - total_unbonded,
            EXPECTED_LOCKED_AMOUNTS[i]
        )
    }

    {
        // Run auction forward by almost a week
        builder.run_auction(WEEK_TIMESTAMPS[13] - 1, Vec::new());

        // Attempt unbond of 1 mote
        expect_undelegate_failure(&mut builder, u64::one());

        // Run auction forward by one millisecond
        builder.run_auction(WEEK_TIMESTAMPS[13], Vec::new());

        // Attempt unbond of released amount + remainder
        expect_undelegate_success(&mut builder, EXPECTED_WEEKLY_RELEASE + EXPECTED_REMAINDER);

        total_unbonded += EXPECTED_WEEKLY_RELEASE + EXPECTED_REMAINDER;

        assert_eq!(
            DELEGATOR_VFTA_STAKE - total_unbonded,
            EXPECTED_LOCKED_AMOUNTS[13]
        )
    }

    assert_eq!(DELEGATOR_VFTA_STAKE, total_unbonded);
}

#[ignore]
#[test]
fn should_reset_delegators_stake_after_slashing() {
    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(SYSTEM_TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_2_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_2),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_2,
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    let delegator_1_validator_2_delegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_2_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    let delegator_2_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_2),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_2_PK.clone(),
        },
    )
    .build();

    let delegator_2_validator_2_delegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_2),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_2_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_2_PK.clone(),
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        validator_1_fund_request,
        validator_2_fund_request,
        validator_1_add_bid_request,
        validator_2_add_bid_request,
        delegator_1_validator_1_delegate_request,
        delegator_1_validator_2_delegate_request,
        delegator_2_validator_1_delegate_request,
        delegator_2_validator_2_delegate_request,
    ];

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).expect_success().commit();
    }

    let auction_hash = builder.get_auction_contract_hash();

    // Check bids before slashing

    let bids_1 = builder.get_bids();
    let _ = bids_1
        .validator_total_stake(&NON_FOUNDER_VALIDATOR_1_PK)
        .expect("should have total stake");

    let validator_1_delegator_stakes_1 = {
        match bids_1.delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK) {
            None => U512::zero(),
            Some(delegators) => delegators.iter().map(|x| x.staked_amount()).sum(),
        }
    };

    assert!(validator_1_delegator_stakes_1 > U512::zero());

    let validator_2_delegator_stakes_1 = {
        match bids_1.delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_2_PK) {
            None => U512::zero(),
            Some(delegators) => delegators.iter().map(|x| x.staked_amount()).sum(),
        }
    };
    assert!(validator_2_delegator_stakes_1 > U512::zero());

    let slash_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        auction_hash,
        auction::METHOD_SLASH,
        runtime_args! {
            auction::ARG_VALIDATOR_PUBLIC_KEYS => vec![
               NON_FOUNDER_VALIDATOR_2_PK.clone(),
            ]
        },
    )
    .build();

    builder.exec(slash_request_1).expect_success().commit();

    // Compare bids after slashing validator 2
    let bids_2 = builder.get_bids();
    assert_ne!(bids_1, bids_2);

    let _ = bids_2
        .validator_bid(&NON_FOUNDER_VALIDATOR_1_PK)
        .expect("should have bids");
    let validator_1_delegator_stakes_2 = {
        match bids_1.delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK) {
            None => U512::zero(),
            Some(delegators) => delegators.iter().map(|x| x.staked_amount()).sum(),
        }
    };
    assert!(validator_1_delegator_stakes_2 > U512::zero());

    assert!(bids_2.validator_bid(&NON_FOUNDER_VALIDATOR_2_PK).is_none());

    // Validator 1 total delegated stake did not change
    assert_eq!(
        validator_1_delegator_stakes_2,
        validator_1_delegator_stakes_1
    );

    let slash_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        auction_hash,
        auction::METHOD_SLASH,
        runtime_args! {
            auction::ARG_VALIDATOR_PUBLIC_KEYS => vec![
                NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ]
        },
    )
    .build();

    builder.exec(slash_request_2).expect_success().commit();

    // Compare bids after slashing validator 2
    let bids_3 = builder.get_bids();
    assert_ne!(bids_3, bids_2);
    assert_ne!(bids_3, bids_1);

    assert!(bids_3.validator_bid(&NON_FOUNDER_VALIDATOR_1_PK).is_none());
    let validator_1_delegator_stakes_3 = {
        match bids_3.delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK) {
            None => U512::zero(),
            Some(delegators) => delegators.iter().map(|x| x.staked_amount()).sum(),
        }
    };

    assert_ne!(
        validator_1_delegator_stakes_3,
        validator_1_delegator_stakes_1
    );
    assert_ne!(
        validator_1_delegator_stakes_3,
        validator_1_delegator_stakes_2
    );

    // Validator 1 total delegated stake is set to 0
    assert_eq!(validator_1_delegator_stakes_3, U512::zero());
}

#[should_panic(expected = "InvalidDelegatedAmount")]
#[ignore]
#[test]
fn should_validate_genesis_delegators_bond_amount() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Some(GenesisValidator::new(Motes::new(ACCOUNT_1_BOND.into()), 80)),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND.into()),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE.into()),
            Motes::new(U512::zero()),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp.push(delegator_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);
}

fn check_validator_slots_for_accounts(accounts: usize) {
    let accounts = {
        let range = 1..=accounts;

        let mut tmp: Vec<GenesisAccount> = Vec::with_capacity(accounts);

        for count in range.map(U256::from) {
            let secret_key = {
                let mut secret_key_bytes = [0; 32];
                count.to_big_endian(&mut secret_key_bytes);
                SecretKey::ed25519_from_bytes(secret_key_bytes).expect("should create ed25519 key")
            };

            let public_key = PublicKey::from(&secret_key);

            let account = GenesisAccount::account(
                public_key,
                Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
                Some(GenesisValidator::new(Motes::new(ACCOUNT_1_BOND.into()), 80)),
            );

            tmp.push(account)
        }

        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);
}

#[should_panic(expected = "InvalidValidatorSlots")]
#[ignore]
#[test]
fn should_fail_with_more_accounts_than_slots() {
    check_validator_slots_for_accounts(DEFAULT_EXEC_CONFIG.validator_slots() as usize + 1);
}

#[ignore]
#[test]
fn should_run_genesis_with_exact_validator_slots() {
    check_validator_slots_for_accounts(DEFAULT_EXEC_CONFIG.validator_slots() as usize);
}

#[ignore]
#[test]
fn should_delegate_and_redelegate() {
    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(SYSTEM_TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_2_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_2),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        delegator_1_fund_request,
        validator_1_fund_request,
        validator_2_fund_request,
        validator_1_add_bid_request,
        validator_2_add_bid_request,
        delegator_1_validator_1_delegate_request,
    ];

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    builder.advance_eras_by_default_auction_delay();

    let delegator_1_undelegate_purse = builder
        .get_entity_by_account_hash(*BID_ACCOUNT_1_ADDR)
        .expect("should have default account")
        .main_purse();

    let delegator_1_redelegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_REDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_1 + DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
            ARG_NEW_VALIDATOR => NON_FOUNDER_VALIDATOR_2_PK.clone()
        },
    )
    .build();

    builder
        .exec(delegator_1_redelegate_request)
        .commit()
        .expect_success();

    let after_redelegation = builder
        .get_unbonds()
        .get(&BID_ACCOUNT_1_ADDR)
        .expect("must have purses")
        .len();

    assert_eq!(1, after_redelegation);

    let delegator_1_purse_balance_before = builder.get_purse_balance(delegator_1_undelegate_purse);

    for _ in 0..=DEFAULT_UNBONDING_DELAY {
        let delegator_1_redelegate_purse_balance =
            builder.get_purse_balance(delegator_1_undelegate_purse);
        assert_eq!(
            delegator_1_purse_balance_before,
            delegator_1_redelegate_purse_balance
        );

        builder.advance_era()
    }

    // Since a redelegation has been processed no funds should have transferred back to the purse.
    let delegator_1_purse_balance_after = builder.get_purse_balance(delegator_1_undelegate_purse);
    assert_eq!(
        delegator_1_purse_balance_before,
        delegator_1_purse_balance_after
    );

    let bids = builder.get_bids();
    assert_eq!(bids.len(), 4);

    let delegators = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK)
        .expect("should have delegators");
    assert_eq!(delegators.len(), 1);
    let delegator = bids
        .delegator_by_public_keys(&NON_FOUNDER_VALIDATOR_1_PK, &BID_ACCOUNT_1_PK)
        .expect("should have delegator");
    let delegated_amount_1 = delegator.staked_amount();
    assert_eq!(
        delegated_amount_1,
        U512::from(DELEGATE_AMOUNT_1 - UNDELEGATE_AMOUNT_1 - DEFAULT_MINIMUM_DELEGATION_AMOUNT)
    );

    let delegators = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_2_PK)
        .expect("should have delegators");
    assert_eq!(delegators.len(), 1);
    let delegator = bids
        .delegator_by_public_keys(&NON_FOUNDER_VALIDATOR_2_PK, &BID_ACCOUNT_1_PK)
        .expect("should have delegator");
    let redelegated_amount_1 = delegator.staked_amount();
    assert_eq!(
        redelegated_amount_1,
        U512::from(UNDELEGATE_AMOUNT_1 + DEFAULT_MINIMUM_DELEGATION_AMOUNT)
    );
}

#[ignore]
#[test]
fn should_handle_redelegation_to_inactive_validator() {
    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_2_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_2),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegator_2_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
        },
    )
    .build();

    let post_genesis_requests = vec![
        validator_1_fund_request,
        validator_2_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        validator_1_add_bid_request,
        validator_2_add_bid_request,
        delegator_1_validator_1_delegate_request,
        delegator_2_validator_1_delegate_request,
    ];

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    builder.advance_eras_by_default_auction_delay();

    let delegator_1_main_purse = builder
        .get_entity_by_account_hash(*DELEGATOR_1_ADDR)
        .expect("should have default account")
        .main_purse();

    let delegator_2_main_purse = builder
        .get_entity_by_account_hash(*DELEGATOR_2_ADDR)
        .expect("should have default account")
        .main_purse();

    let invalid_redelegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_REDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_1 + DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
            ARG_NEW_VALIDATOR => BID_ACCOUNT_1_PK.clone()
        },
    )
    .build();

    builder
        .exec(invalid_redelegate_request)
        .expect_success()
        .commit();

    builder.advance_era();

    let valid_redelegate_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_REDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_1 + DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
            ARG_NEW_VALIDATOR => NON_FOUNDER_VALIDATOR_2_PK.clone()
        },
    )
    .build();

    builder
        .exec(valid_redelegate_request)
        .expect_success()
        .commit();

    let delegator_1_purse_balance_before = builder.get_purse_balance(delegator_1_main_purse);
    let delegator_2_purse_balance_before = builder.get_purse_balance(delegator_2_main_purse);

    for _ in 0..=DEFAULT_UNBONDING_DELAY {
        let delegator_2_purse_balance = builder.get_purse_balance(delegator_2_main_purse);
        assert_eq!(delegator_2_purse_balance, delegator_2_purse_balance_before);

        builder.advance_era();
    }

    // The invalid redelegation will force an unbond which will transfer funds to
    // back to the main purse.
    let delegator_1_purse_balance_after = builder.get_purse_balance(delegator_1_main_purse);
    assert_eq!(
        delegator_1_purse_balance_before
            + U512::from(UNDELEGATE_AMOUNT_1 + DEFAULT_MINIMUM_DELEGATION_AMOUNT),
        delegator_1_purse_balance_after
    );

    // The valid redelegation will not transfer funds back to the main purse.
    let delegator_2_purse_balance_after = builder.get_purse_balance(delegator_2_main_purse);
    assert_eq!(
        delegator_2_purse_balance_before,
        delegator_2_purse_balance_after
    );
}

#[ignore]
#[test]
fn should_enforce_minimum_delegation_amount() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let transfer_to_validator_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let transfer_to_delegator_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(BID_ACCOUNT_1_BALANCE)
        },
    )
    .build();

    let post_genesis_request = vec![transfer_to_validator_1, transfer_to_delegator_1];

    for request in post_genesis_request {
        builder.exec(request).expect_success().commit();
    }

    let add_bid_request_1 = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(add_bid_request_1).expect_success().commit();

    for _ in 0..=builder.get_auction_delay() {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(builder.get_era().successor())
            .with_run_auction(true)
            .build();

        builder
            .step(step_request)
            .expect("must execute step request");
    }

    let delegation_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(100u64),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    // The delegation amount is below the default value of 500 CSPR,
    // therefore the delegation should not succeed.
    builder.exec(delegation_request_1).expect_failure();

    let error = builder.get_error().expect("must get error");
    assert!(matches!(
        error,
        Error::Exec(execution::Error::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::DelegationAmountTooSmall as u8));
}

#[ignore]
#[test]
fn should_allow_delegations_with_minimal_floor_amount() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let transfer_to_validator_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let transfer_to_delegator_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(BID_ACCOUNT_1_BALANCE)
        },
    )
    .build();

    let transfer_to_delegator_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_2_ADDR,
            ARG_AMOUNT => U512::from(BID_ACCOUNT_1_BALANCE)
        },
    )
    .build();

    let post_genesis_request = vec![
        transfer_to_validator_1,
        transfer_to_delegator_1,
        transfer_to_delegator_2,
    ];

    for request in post_genesis_request {
        builder.exec(request).expect_success().commit();
    }

    let add_bid_request_1 = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(add_bid_request_1).expect_success().commit();

    for _ in 0..=builder.get_auction_delay() {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(builder.get_era().successor())
            .with_run_auction(true)
            .build();

        builder
            .step(step_request)
            .expect("must execute step request");
    }

    let delegation_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT - 1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    // The delegation amount is below the default value of 500 CSPR,
    // therefore the delegation should not succeed.
    builder.exec(delegation_request_1).expect_failure();

    let error = builder.get_error().expect("must get error");

    assert!(matches!(
        error,
        Error::Exec(execution::Error::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::DelegationAmountTooSmall as u8));

    let delegation_request_2 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_2_PK.clone(),
        },
    )
    .build();

    builder.exec(delegation_request_2).expect_success().commit();
}

#[ignore]
#[test]
fn should_enforce_max_delegators_per_validator_cap() {
    let engine_config = EngineConfigBuilder::new()
        .with_max_delegators_per_validator(Some(2u32))
        .build();

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new_with_config(data_dir.path(), engine_config);

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let transfer_to_validator_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let transfer_to_delegator_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(BID_ACCOUNT_1_BALANCE)
        },
    )
    .build();

    let transfer_to_delegator_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_2_ADDR,
            ARG_AMOUNT => U512::from(BID_ACCOUNT_1_BALANCE)
        },
    )
    .build();

    let transfer_to_delegator_3 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(BID_ACCOUNT_1_BALANCE)
        },
    )
    .build();

    let post_genesis_request = vec![
        transfer_to_validator_1,
        transfer_to_delegator_1,
        transfer_to_delegator_2,
        transfer_to_delegator_3,
    ];

    for request in post_genesis_request {
        builder.exec(request).expect_success().commit();
    }

    let add_bid_request_1 = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(add_bid_request_1).expect_success().commit();

    for _ in 0..=builder.get_auction_delay() {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(builder.get_era().successor())
            .with_run_auction(true)
            .build();

        builder
            .step(step_request)
            .expect("must execute step request");
    }

    let delegation_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    let delegation_request_2 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_2_PK.clone(),
        },
    )
    .build();

    let delegation_requests = [delegation_request_1, delegation_request_2];

    for request in delegation_requests {
        builder.exec(request).expect_success().commit();
    }

    let delegation_request_3 = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    builder.exec(delegation_request_3).expect_failure();

    let error = builder.get_error().expect("must get error");

    assert!(matches!(
        error,
        Error::Exec(execution::Error::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ExceededDelegatorSizeLimit as u8));

    let delegator_2_staked_amount = {
        let bids = builder.get_bids();
        let delegator = bids
            .delegator_by_public_keys(&NON_FOUNDER_VALIDATOR_1_PK, &BID_ACCOUNT_2_PK)
            .expect("should have delegator bid");
        delegator.staked_amount()
    };

    let undelegation_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_2_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            ARG_AMOUNT => delegator_2_staked_amount,
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_2_PK.clone(),
        },
    )
    .build();

    builder.exec(undelegation_request).expect_success().commit();

    let bids = builder.get_bids();

    let current_delegator_count = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK)
        .expect("must have bid record")
        .iter()
        .filter(|x| x.staked_amount() > U512::zero())
        .collect::<Vec<&auction::Delegator>>()
        .len();

    assert_eq!(current_delegator_count, 1);

    let delegation_request_3 = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    builder.exec(delegation_request_3).expect_success().commit();

    let bids = builder.get_bids();
    let current_delegator_count = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK)
        .expect("must have bid record")
        .len();

    assert_eq!(current_delegator_count, 2);
}

#[ignore]
#[test]
fn should_transfer_to_main_purse_in_case_of_redelegation_past_max_delegation_cap() {
    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let transfer_to_delegator_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(BID_ACCOUNT_1_BALANCE)
        },
    )
    .build();

    let transfer_to_delegator_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_2_ADDR,
            ARG_AMOUNT => U512::from(BID_ACCOUNT_1_BALANCE)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_2_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_2),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    let delegator_1_validator_2_delegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_2_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_2_PK.clone(),
        },
    )
    .build();

    let post_genesis_requests = vec![
        validator_1_fund_request,
        validator_2_fund_request,
        transfer_to_delegator_1,
        transfer_to_delegator_2,
        validator_1_add_bid_request,
        validator_2_add_bid_request,
        delegator_1_validator_1_delegate_request,
        delegator_1_validator_2_delegate_request,
    ];

    let engine_config = EngineConfigBuilder::new()
        .with_max_delegators_per_validator(Some(1u32))
        .build();

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new_with_config(data_dir.path(), engine_config);

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).expect_success().commit();
    }

    builder.advance_eras_by_default_auction_delay();

    let delegator_1_main_purse = builder
        .get_entity_by_account_hash(*BID_ACCOUNT_1_ADDR)
        .expect("should have default account")
        .main_purse();

    let delegator_1_redelegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_REDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_1 + DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
            ARG_NEW_VALIDATOR => NON_FOUNDER_VALIDATOR_2_PK.clone()
        },
    )
    .build();

    builder
        .exec(delegator_1_redelegate_request)
        .commit()
        .expect_success();

    let after_redelegation = builder
        .get_unbonds()
        .get(&BID_ACCOUNT_1_ADDR)
        .expect("must have purses")
        .len();

    assert_eq!(1, after_redelegation);

    let delegator_1_purse_balance_before = builder.get_purse_balance(delegator_1_main_purse);

    for _ in 0..=DEFAULT_UNBONDING_DELAY {
        let delegator_1_redelegate_purse_balance =
            builder.get_purse_balance(delegator_1_main_purse);
        assert_eq!(
            delegator_1_purse_balance_before,
            delegator_1_redelegate_purse_balance
        );

        builder.advance_era();
    }

    let delegator_1_purse_balance_after = builder.get_purse_balance(delegator_1_main_purse);

    assert_eq!(
        delegator_1_purse_balance_before
            + U512::from(UNDELEGATE_AMOUNT_1 + DEFAULT_MINIMUM_DELEGATION_AMOUNT),
        delegator_1_purse_balance_after
    )
}

#[ignore]
#[test]
fn should_delegate_and_redelegate_with_eviction_regression_test() {
    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(SYSTEM_TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_2_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_2_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let validator_2_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_2_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_2),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    let delegator_1_validator_1_delegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        delegator_1_fund_request,
        validator_1_fund_request,
        validator_2_fund_request,
        validator_1_add_bid_request,
        validator_2_add_bid_request,
        delegator_1_validator_1_delegate_request,
    ];

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    let delegator_1_redelegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_REDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
            ARG_NEW_VALIDATOR => NON_FOUNDER_VALIDATOR_2_PK.clone()
        },
    )
    .build();

    builder
        .exec(delegator_1_redelegate_request)
        .commit()
        .expect_success();

    builder.advance_eras_by(DEFAULT_UNBONDING_DELAY);

    // Advance one more era, this is the point where the redelegate request is processed (era >=
    // unbonding_delay + 1)
    builder.advance_era();

    let bids = builder.get_bids();
    assert!(bids
        .delegator_by_public_keys(&NON_FOUNDER_VALIDATOR_1_PK, &BID_ACCOUNT_1_PK)
        .is_none());
    assert!(bids
        .delegator_by_public_keys(&NON_FOUNDER_VALIDATOR_2_PK, &BID_ACCOUNT_1_PK)
        .is_some());
}

#[ignore]
#[test]
fn should_increase_existing_delegation_when_limit_exceeded() {
    let engine_config = EngineConfigBuilder::default()
        .with_max_delegators_per_validator(Some(2))
        .build();

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new_with_config(data_dir.path(), engine_config);

    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let transfer_to_validator_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let transfer_to_delegator_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(BID_ACCOUNT_1_BALANCE)
        },
    )
    .build();

    let transfer_to_delegator_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *BID_ACCOUNT_2_ADDR,
            ARG_AMOUNT => U512::from(BID_ACCOUNT_1_BALANCE)
        },
    )
    .build();

    let transfer_to_delegator_3 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(BID_ACCOUNT_1_BALANCE)
        },
    )
    .build();

    let post_genesis_request = vec![
        transfer_to_validator_1,
        transfer_to_delegator_1,
        transfer_to_delegator_2,
        transfer_to_delegator_3,
    ];

    for request in post_genesis_request {
        builder.exec(request).expect_success().commit();
    }

    let add_bid_request_1 = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(add_bid_request_1).expect_success().commit();

    for _ in 0..=builder.get_auction_delay() {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(builder.get_era().successor())
            .with_run_auction(true)
            .build();

        builder
            .step(step_request)
            .expect("must execute step request");
    }

    let delegation_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    let delegation_request_2 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_2_PK.clone(),
        },
    )
    .build();

    let delegation_requests = [delegation_request_1, delegation_request_2];

    for request in delegation_requests {
        builder.exec(request).expect_success().commit();
    }

    let delegation_request_3 = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    builder.exec(delegation_request_3).expect_failure();

    let error = builder.get_error().expect("must get error");

    assert!(matches!(
        error,
        Error::Exec(execution::Error::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ExceededDelegatorSizeLimit as u8));

    // The validator already has the maximum number of delegators allowed. However, this is a
    // delegator that already delegated, so their bid should just be increased.
    let delegation_request_2_repeat = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_2_PK.clone(),
        },
    )
    .build();

    builder
        .exec(delegation_request_2_repeat)
        .expect_success()
        .commit();
}
