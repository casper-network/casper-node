use assert_matches::assert_matches;
use num_traits::{One, Zero};
use once_cell::sync::Lazy;
use std::{
    collections::{BTreeMap, BTreeSet},
    iter::FromIterator,
};
use tempfile::TempDir;

use casper_engine_test_support::{
    genesis_config_builder::GenesisConfigBuilder, utils, ChainspecConfig, ExecuteRequestBuilder,
    LmdbWasmTestBuilder, StepRequestBuilder, UpgradeRequestBuilder, DEFAULT_ACCOUNTS,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_AUCTION_DELAY,
    DEFAULT_CHAINSPEC_REGISTRY, DEFAULT_EXEC_CONFIG, DEFAULT_GENESIS_CONFIG_HASH,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
    DEFAULT_MAXIMUM_DELEGATION_AMOUNT, DEFAULT_PROTOCOL_VERSION, DEFAULT_ROUND_SEIGNIORAGE_RATE,
    DEFAULT_UNBONDING_DELAY, LOCAL_GENESIS_REQUEST, MINIMUM_ACCOUNT_CREATION_BALANCE, SYSTEM_ADDR,
    TIMESTAMP_MILLIS_INCREMENT,
};
use casper_execution_engine::{
    engine_state::{engine_config::DEFAULT_MINIMUM_DELEGATION_AMOUNT, Error},
    execution::ExecError,
};
use casper_storage::data_access_layer::{GenesisRequest, HandleFeeMode};

use crate::lmdb_fixture;
use casper_types::{
    self,
    account::AccountHash,
    api_error::ApiError,
    runtime_args,
    system::auction::{
        self, BidKind, BidsExt, DelegationRate, DelegatorKind, EraValidators,
        Error as AuctionError, UnbondKind, ValidatorWeights, ARG_AMOUNT, ARG_DELEGATION_RATE,
        ARG_DELEGATOR, ARG_ENTRY_POINT, ARG_MAXIMUM_DELEGATION_AMOUNT,
        ARG_MINIMUM_DELEGATION_AMOUNT, ARG_NEW_PUBLIC_KEY, ARG_NEW_VALIDATOR, ARG_PUBLIC_KEY,
        ARG_REWARDS_MAP, ARG_VALIDATOR, ERA_ID_KEY, INITIAL_ERA_ID, METHOD_DISTRIBUTE,
    },
    EntityAddr, EraId, GenesisAccount, GenesisValidator, HoldBalanceHandling, Key, Motes,
    ProtocolVersion, PublicKey, SecretKey, TransactionHash, DEFAULT_MINIMUM_BID_AMOUNT, U256, U512,
};

const ARG_TARGET: &str = "target";

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_ACTIVATE_BID: &str = "activate_bid.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";
const CONTRACT_WITHDRAW_BID: &str = "withdraw_bid.wasm";
const CONTRACT_DELEGATE: &str = "delegate.wasm";
const CONTRACT_UNDELEGATE: &str = "undelegate.wasm";
const CONTRACT_REDELEGATE: &str = "redelegate.wasm";
const CONTRACT_CHANGE_BID_PUBLIC_KEY: &str = "change_bid_public_key.wasm";

const TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE + 1000;

const ADD_BID_AMOUNT_1: u64 = 95_000;
const ADD_BID_AMOUNT_2: u64 = 47_500;
const ADD_BID_AMOUNT_3: u64 = 200_000;
const ADD_BID_DELEGATION_RATE_1: DelegationRate = 10;
const BID_AMOUNT_2: u64 = 5_000;
const ADD_BID_DELEGATION_RATE_2: DelegationRate = 15;
const WITHDRAW_BID_AMOUNT_2: u64 = 15_000;
const ADD_BID_DELEGATION_RATE_3: DelegationRate = 20;

const DELEGATE_AMOUNT_1: u64 = 125_000 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const DELEGATE_AMOUNT_2: u64 = 15_000 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const UNDELEGATE_AMOUNT_1: u64 = 35_000;
const UNDELEGATE_AMOUNT_2: u64 = 5_000;

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

static NON_FOUNDER_VALIDATOR_3_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([5; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static NON_FOUNDER_VALIDATOR_3_ADDR: Lazy<AccountHash> =
    Lazy::new(|| AccountHash::from(&*NON_FOUNDER_VALIDATOR_3_PK));

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
const BID_ACCOUNT_1_BOND: u64 = 200_000;

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
            Motes::new(BID_ACCOUNT_1_BALANCE),
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
fn should_add_new_bid_with_limits() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE),
            None,
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let exec_request_0 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
            // Below global minimum.
            ARG_MINIMUM_DELEGATION_AMOUNT => 1_000_000_000u64,
        },
    )
    .build();

    builder.exec(exec_request_0).expect_failure();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
            ARG_MINIMUM_DELEGATION_AMOUNT => 600_000_000_000u64,
            ARG_MAXIMUM_DELEGATION_AMOUNT => 900_000_000_000u64,
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
            Motes::new(BID_ACCOUNT_1_BALANCE),
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
            Motes::new(BID_ACCOUNT_1_BALANCE),
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

    let bids: Vec<_> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();

    assert_eq!(bids.len(), 1);

    let active_bid = bids.validator_bid(&BID_ACCOUNT_1_PK.clone()).unwrap();
    assert_eq!(
        builder.get_purse_balance(*active_bid.bonding_purse()),
        // Since we don't pay out immediately `WITHDRAW_BID_AMOUNT_2` is locked in unbonding queue
        U512::from(ADD_BID_AMOUNT_1)
    );
    let unbonds = builder.get_unbonds();
    let unbond_kind = UnbondKind::Validator(BID_ACCOUNT_1_PK.clone());
    let unbonds = unbonds.get(&unbond_kind).expect("should have unbonded");
    let unbond = unbonds.first().expect("must have at least an unbond");
    assert_eq!(unbond.eras().len(), 1);
    assert_eq!(unbond.unbond_kind(), &unbond_kind);
    assert_eq!(unbond.validator_public_key(), &*BID_ACCOUNT_1_PK);

    let era = unbond.eras().first().expect("should have era");
    // `WITHDRAW_BID_AMOUNT_2` is in unbonding list
    assert_eq!(era.amount(), &U512::from(WITHDRAW_BID_AMOUNT_2),);
    assert_eq!(era.era_of_creation(), INITIAL_ERA_ID,);
}

#[ignore]
#[test]
fn should_run_delegate_and_undelegate() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE),
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

    let bids: Vec<BidKind> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();
    assert_eq!(bids.len(), 1);
    let active_bid = bids.validator_bid(&NON_FOUNDER_VALIDATOR_1_PK).unwrap();
    assert_eq!(
        builder.get_purse_balance(*active_bid.bonding_purse()),
        U512::from(ADD_BID_AMOUNT_1)
    );
    assert_eq!(*active_bid.delegation_rate(), ADD_BID_DELEGATION_RATE_1);

    let auction_key = Key::Hash(auction_hash.value());

    let auction_stored_value = builder
        .query(None, auction_key, &[])
        .expect("should query auction hash");
    let _auction = auction_stored_value
        .as_contract()
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

    let bids: Vec<_> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();
    assert_eq!(bids.len(), 2);
    let delegators = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK)
        .expect("should have delegators");
    assert_eq!(delegators.len(), 1);
    let delegator = bids
        .delegator_by_kind(
            &NON_FOUNDER_VALIDATOR_1_PK,
            &DelegatorKind::PublicKey(BID_ACCOUNT_1_PK.clone()),
        )
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

    let bids: Vec<_> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();
    assert_eq!(bids.len(), 2);
    let delegators = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK)
        .expect("should have delegators");
    assert_eq!(delegators.len(), 1);
    let delegator = bids
        .delegator_by_kind(
            &NON_FOUNDER_VALIDATOR_1_PK,
            &DelegatorKind::PublicKey(BID_ACCOUNT_1_PK.clone()),
        )
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

    let bids: Vec<_> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();
    assert_eq!(bids.len(), 2);
    let delegators = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK)
        .expect("should have delegators");
    assert_eq!(delegators.len(), 1);
    let delegator = bids
        .delegator_by_kind(
            &NON_FOUNDER_VALIDATOR_1_PK,
            &DelegatorKind::PublicKey(BID_ACCOUNT_1_PK.clone()),
        )
        .expect("should have account1 delegation");
    let delegated_amount_1 = delegator.staked_amount();
    assert_eq!(
        delegated_amount_1,
        U512::from(DELEGATE_AMOUNT_1 + DELEGATE_AMOUNT_2 - UNDELEGATE_AMOUNT_1)
    );

    let unbonding_purses = builder.get_unbonds();
    assert_eq!(unbonding_purses.len(), 1);

    let unbond_kind = UnbondKind::DelegatedPublicKey(BID_ACCOUNT_1_PK.clone());
    let unbond = unbonding_purses
        .get(&unbond_kind)
        .expect("should have unbonding purse for non founder validator");
    let unbond = unbond.first().expect("must get unbond");
    assert_eq!(unbond.eras().len(), 1);
    assert_eq!(unbond.validator_public_key(), &*NON_FOUNDER_VALIDATOR_1_PK);
    assert_eq!(unbond.unbond_kind(), &unbond_kind);
    assert!(!unbond.is_validator());
    let era = unbond.eras().first().expect("should have era");
    assert_eq!(era.amount(), &U512::from(UNDELEGATE_AMOUNT_1));
    assert_eq!(era.era_of_creation(), INITIAL_ERA_ID);
}

#[ignore]
#[test]
fn should_run_delegate_with_delegation_amount_limits() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE),
            None,
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let transfer_request = ExecuteRequestBuilder::standard(
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
            ARG_MINIMUM_DELEGATION_AMOUNT => DELEGATE_AMOUNT_1,
            ARG_MAXIMUM_DELEGATION_AMOUNT => DELEGATE_AMOUNT_1,
        },
    )
    .build();

    builder.exec(transfer_request).expect_success().commit();
    builder.exec(add_bid_request_1).expect_success().commit();

    let bids = builder.get_bids();
    assert_eq!(bids.len(), 1);
    let active_bid = bids.validator_bid(&NON_FOUNDER_VALIDATOR_1_PK).unwrap();
    assert_eq!(
        builder.get_purse_balance(*active_bid.bonding_purse()),
        U512::from(ADD_BID_AMOUNT_1)
    );
    assert_eq!(*active_bid.delegation_rate(), ADD_BID_DELEGATION_RATE_1);

    let exec_request_0 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1 - 1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    builder.exec(exec_request_0).expect_failure();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1 + 1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    builder.exec(exec_request_1).expect_failure();

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
        },
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();
}

#[ignore]
#[test]
fn should_forcibly_undelegate_after_setting_validator_limits() {
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
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_2),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
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
        delegator_1_validator_1_delegate_request,
        delegator_2_validator_1_delegate_request,
    ];

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    // builder.advance_eras_by_default_auction_delay();

    let bids: Vec<_> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();
    assert_eq!(bids.len(), 3);

    let auction_delay = builder.get_auction_delay();
    // new_era is the first era in the future where new era validator weights will be calculated
    let new_era = INITIAL_ERA_ID + auction_delay + 1;
    assert!(builder.get_validator_weights(new_era).is_none());
    assert_eq!(
        builder.get_validator_weights(new_era - 1).unwrap(),
        builder.get_validator_weights(INITIAL_ERA_ID).unwrap()
    );

    builder.run_auction(
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
        Vec::new(),
    );

    let validator_weights: ValidatorWeights = builder
        .get_validator_weights(new_era)
        .expect("should have first era validator weights");

    assert_eq!(
        *validator_weights.get(&NON_FOUNDER_VALIDATOR_1_PK).unwrap(),
        U512::from(ADD_BID_AMOUNT_1 + DELEGATE_AMOUNT_1 + DELEGATE_AMOUNT_2)
    );

    // set delegation limits
    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_AMOUNT => U512::from(1_000),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
            ARG_MINIMUM_DELEGATION_AMOUNT => DELEGATE_AMOUNT_2 + 1_000,
            ARG_MAXIMUM_DELEGATION_AMOUNT => DELEGATE_AMOUNT_1 - 1_000,
        },
    )
    .build();

    builder
        .exec(validator_1_add_bid_request)
        .expect_success()
        .commit();

    let bids: Vec<_> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();
    assert_eq!(bids.len(), 2);

    let bids: Vec<_> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();
    assert_eq!(bids.len(), 2);

    assert!(builder.get_validator_weights(new_era + 1).is_none());

    builder.run_auction(
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
        Vec::new(),
    );

    let validator_weights: ValidatorWeights = builder
        .get_validator_weights(new_era + 1)
        .expect("should have first era validator weights");

    assert_eq!(
        *validator_weights.get(&NON_FOUNDER_VALIDATOR_1_PK).unwrap(),
        // The validator has now bid ADD_BID_AMOUNT_1 + 1_000.
        // Delegator 1's delegation has been decreased to the maximum of DELEGATE_AMOUNT_1 - 1_000.
        // Delegator 2's delegation was below minimum, so it has been completely unbonded.
        U512::from(ADD_BID_AMOUNT_1 + 1_000 + DELEGATE_AMOUNT_1 - 1_000)
    );

    let unbonding_purses = builder.get_unbonds();
    let unbond_kind = UnbondKind::DelegatedPublicKey(DELEGATOR_1.clone());
    let delegator_1 = unbonding_purses
        .get(&unbond_kind)
        .expect("should have delegator_1")
        .first()
        .expect("must get unbond");

    let delegator_1_unbonding = delegator_1
        .eras()
        .first()
        .expect("should have delegator_1 unbonding");

    let overage = 1_000;

    assert_eq!(
        delegator_1_unbonding.amount(),
        &U512::from(overage),
        "expected delegator_1 amount to match"
    );

    let unbond_kind = UnbondKind::DelegatedPublicKey(DELEGATOR_2.clone());
    let delegator_2 = unbonding_purses
        .get(&unbond_kind)
        .expect("should have delegator_2");

    let delegator_2_unbonding = delegator_2
        .first()
        .expect("must have unbond")
        .eras()
        .first()
        .expect("should have era");

    assert_eq!(
        delegator_2_unbonding.amount(),
        &U512::from(DELEGATE_AMOUNT_2),
        "expected delegator_2 amount to match"
    );
}

#[ignore]
#[test]
fn should_not_allow_delegator_stake_range_during_vesting() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(BID_ACCOUNT_1_BOND),
                DelegationRate::zero(),
            )),
        );
        tmp.push(account_1);
        tmp
    };

    let genesis_config = GenesisConfigBuilder::new()
        .with_accounts(accounts)
        .with_locked_funds_period_millis(1)
        .build();

    let run_genesis_request = GenesisRequest::new(
        DEFAULT_GENESIS_CONFIG_HASH,
        DEFAULT_PROTOCOL_VERSION,
        genesis_config,
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    );

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);
    // need to step past genesis era
    builder.advance_era();

    // attempt to change delegation limits
    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK.clone(),
            ARG_AMOUNT => U512::from(ACCOUNT_1_BOND),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
            ARG_MINIMUM_DELEGATION_AMOUNT => DEFAULT_MINIMUM_DELEGATION_AMOUNT,
            ARG_MAXIMUM_DELEGATION_AMOUNT => DEFAULT_MAXIMUM_DELEGATION_AMOUNT - 2,
        },
    )
    .build();

    builder.exec(validator_1_add_bid_request).expect_failure();

    let error = builder.get_error().expect("must have error");
    let err_str = format!("{}", error);
    assert!(
        err_str.starts_with("ApiError::AuctionError(VestingLockout)"),
        "should get vesting lockout error"
    );
}

#[ignore]
#[test]
fn should_allow_delegator_stake_range_change_if_no_vesting() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(BID_ACCOUNT_1_BOND),
                DelegationRate::zero(),
            )),
        );
        tmp.push(account_1);
        tmp
    };

    let genesis_config = GenesisConfigBuilder::new()
        .with_accounts(accounts)
        .with_locked_funds_period_millis(0)
        .build();

    let run_genesis_request = GenesisRequest::new(
        DEFAULT_GENESIS_CONFIG_HASH,
        DEFAULT_PROTOCOL_VERSION,
        genesis_config,
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    );

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);
    // need to step past genesis era
    builder.advance_era();

    // attempt to change delegation limits
    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK.clone(),
            ARG_AMOUNT => U512::from(ACCOUNT_1_BOND),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
            ARG_MINIMUM_DELEGATION_AMOUNT => DEFAULT_MINIMUM_DELEGATION_AMOUNT,
            ARG_MAXIMUM_DELEGATION_AMOUNT => DEFAULT_MAXIMUM_DELEGATION_AMOUNT - 2,
        },
    )
    .build();

    builder
        .exec(validator_1_add_bid_request)
        .expect_success()
        .commit();
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
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND),
                DelegationRate::zero(),
            )),
        );
        let account_3 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE),
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

    let snapshot_size = auction_delay as usize + 2;
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
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND),
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
        DEFAULT_GENESIS_CONFIG_HASH,
        DEFAULT_PROTOCOL_VERSION,
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
    let snapshot_size = auction_delay as usize + 2;

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

        let error = builder
            .get_last_exec_result()
            .expect("should have last exec result")
            .error()
            .cloned()
            .expect("should have error");
        assert_matches!(
            error,
            Error::Exec(ExecError::Revert(ApiError::AuctionError(15)))
        );
    };

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND),
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
            DEFAULT_GENESIS_CONFIG_HASH,
            DEFAULT_PROTOCOL_VERSION,
            exec_config,
            DEFAULT_CHAINSPEC_REGISTRY.clone(),
        )
    };

    let chainspec = ChainspecConfig::default()
        .with_minimum_delegation_amount(NEW_MINIMUM_DELEGATION_AMOUNT)
        .with_vesting_schedule_period_millis(CASPER_VESTING_SCHEDULE_PERIOD_MILLIS);

    let mut builder = LmdbWasmTestBuilder::new_temporary_with_config(chainspec);

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
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND),
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
        Motes::new(ACCOUNT_1_BALANCE),
        Some(GenesisValidator::new(
            Motes::new(ACCOUNT_1_BOND),
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
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND),
                DelegationRate::zero(),
            )),
        );
        let account_3 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE),
            None,
        );
        let account_4 = GenesisAccount::account(
            BID_ACCOUNT_2_PK.clone(),
            Motes::new(BID_ACCOUNT_2_BALANCE),
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
        BTreeSet::from_iter(vec![BID_ACCOUNT_1_PK.clone(), BID_ACCOUNT_2_PK.clone()])
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

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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
    const VALIDATOR_1_REMAINING_BID: u64 = DEFAULT_MINIMUM_BID_AMOUNT;
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

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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
    let delegator_kinds = delegators
        .iter()
        .map(|x| x.delegator_kind())
        .cloned()
        .collect::<BTreeSet<DelegatorKind>>();
    assert_eq!(
        delegator_kinds,
        BTreeSet::from_iter(vec![
            DelegatorKind::PublicKey(DELEGATOR_1.clone()),
            DelegatorKind::PublicKey(DELEGATOR_2.clone())
        ])
    );

    // Validator partially unbonds and only one entry is present
    let unbonding_purses_before = builder.get_unbonds();
    let unbond_kind = UnbondKind::Validator(VALIDATOR_1.clone());
    let unbond = unbonding_purses_before[&unbond_kind]
        .first()
        .expect("must get unbond");
    assert_eq!(unbond.eras().len(), 1);
    let unbond = &unbonding_purses_before[&unbond_kind]
        .first()
        .expect("must have unbond");
    assert_eq!(
        unbond.unbond_kind(),
        &UnbondKind::Validator(VALIDATOR_1.clone())
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

    let unbonding_purses_after = builder.get_unbonds();
    assert_ne!(unbonding_purses_after, unbonding_purses_before);

    let unbond_kind = UnbondKind::Validator(VALIDATOR_1.clone());
    let validator1 = unbonding_purses_after
        .get(&unbond_kind)
        .expect("should have validator1")
        .first()
        .expect("must have unbond");

    let validator1_unbonding = validator1.eras().first().expect("should have eras");

    assert_eq!(
        validator1_unbonding.amount(),
        &U512::from(VALIDATOR_1_WITHDRAW_AMOUNT),
        "expected validator1 amount to match"
    );

    let unbond_kind = UnbondKind::DelegatedPublicKey(DELEGATOR_1.clone());
    let delegator1 = unbonding_purses_after
        .get(&unbond_kind)
        .expect("should have delegator1")
        .first()
        .expect("must have unbond");

    let delegator1_unbonding = delegator1.eras().first().expect("should have eras");

    assert_eq!(
        delegator1_unbonding.amount(),
        &U512::from(DELEGATOR_1_STAKE),
        "expected delegator1 amount to match"
    );

    let unbond_kind = UnbondKind::DelegatedPublicKey(DELEGATOR_2.clone());
    let delegator2 = unbonding_purses_after
        .get(&unbond_kind)
        .expect("should have delegator2")
        .first()
        .expect("must have unbond");

    let delegator2_unbonding = delegator2.eras().first().expect("should have eras");

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

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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

    let unbonding_purses_before = builder.get_unbonds();

    let unbond_kind = UnbondKind::Validator(VALIDATOR_1.clone());
    let validator_1_era = unbonding_purses_before
        .get(&unbond_kind)
        .expect("should have unbonding purse")
        .first()
        .expect("must have unbond")
        .eras()
        .first()
        .expect("should have era");

    let unbond_kind = UnbondKind::DelegatedPublicKey(DELEGATOR_1.clone());
    let delegator_1_unbonding_purse = unbonding_purses_before
        .get(&unbond_kind)
        .expect("should have unbonding purse entry")
        .first()
        .expect("must have unbond")
        .eras()
        .first()
        .expect("should have unbonding purse");

    let unbond_kind = UnbondKind::DelegatedPublicKey(DELEGATOR_2.clone());
    let delegator_2_unbonding_purse = unbonding_purses_before
        .get(&unbond_kind)
        .expect("should have unbonding purse entry")
        .first()
        .expect("must have unbond")
        .eras()
        .first()
        .expect("should have unbonding purse");

    assert_eq!(validator_1_era.amount(), &U512::from(VALIDATOR_1_STAKE));
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
fn should_undelegate_delegators_when_validator_unbonds_below_minimum_bid_amount() {
    const VALIDATOR_1_REMAINING_BID: u64 = DEFAULT_MINIMUM_BID_AMOUNT - 1;
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

    let post_genesis_requests = vec![
        system_fund_request,
        validator_1_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        validator_1_add_bid_request,
        delegator_1_delegate_request,
        delegator_2_delegate_request,
    ];

    let mut timestamp_millis = DEFAULT_GENESIS_TIMESTAMP_MILLIS;
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    // Try to unbond partially. Stake would fall below minimum bid which should force a full
    // unbonding.
    let validator_1_withdraw_bid = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
            ARG_AMOUNT => U512::from(VALIDATOR_1_WITHDRAW_AMOUNT),
        },
    )
    .build();

    builder
        .exec(validator_1_withdraw_bid)
        .commit()
        .expect_success();

    let bids_after = builder.get_bids();
    assert!(bids_after.validator_bid(&VALIDATOR_1).is_none());

    let unbonding_purses_before = builder.get_unbonds();

    let unbond_kind = UnbondKind::Validator(VALIDATOR_1.clone());
    let validator_1_era = unbonding_purses_before
        .get(&unbond_kind)
        .expect("should have unbonding purse")
        .first()
        .expect("must have unbond")
        .eras()
        .first()
        .expect("should have era");

    let unbond_kind = UnbondKind::DelegatedPublicKey(DELEGATOR_1.clone());
    let delegator_1_unbonding_purse = unbonding_purses_before
        .get(&unbond_kind)
        .expect("should have unbonding purse entry")
        .first()
        .expect("must have unbond")
        .eras()
        .first()
        .expect("should have unbonding purse");

    let unbond_kind = UnbondKind::DelegatedPublicKey(DELEGATOR_2.clone());
    let delegator_2_unbonding_purse = unbonding_purses_before
        .get(&unbond_kind)
        .expect("should have unbonding purse entry")
        .first()
        .expect("must have unbond")
        .eras()
        .first()
        .expect("should have unbonding purse");

    assert_eq!(validator_1_era.amount(), &U512::from(VALIDATOR_1_STAKE));
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
        const ARG_VALIDATOR: &str = "validator";
        let run_request = ExecuteRequestBuilder::standard(
            AccountHash::from(&validator_public_key),
            CONTRACT_ACTIVATE_BID,
            runtime_args! {
                ARG_VALIDATOR => validator_public_key,
            },
        )
        .build();
        builder.exec(run_request).expect_success().commit();
    };

    let latest_validators = |builder: &mut LmdbWasmTestBuilder| {
        let era_validators: EraValidators = builder.get_era_validators();
        let validators = era_validators
            .iter()
            .next_back()
            .map(|(_era_id, validators)| validators)
            .expect("should have validators");
        validators.keys().cloned().collect::<BTreeSet<PublicKey>>()
    };

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND),
                DelegationRate::zero(),
            )),
        );
        let account_3 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(300_000),
                DelegationRate::zero(),
            )),
        );
        let account_4 = GenesisAccount::account(
            BID_ACCOUNT_2_PK.clone(),
            Motes::new(BID_ACCOUNT_2_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(400_000),
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
            BID_ACCOUNT_2_PK.clone(),
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
        BTreeSet::from_iter(vec![ACCOUNT_1_PK.clone(), ACCOUNT_2_PK.clone()])
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
            BID_ACCOUNT_1_PK.clone(),
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
            BID_ACCOUNT_2_PK.clone(),
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
            BID_ACCOUNT_2_PK.clone(),
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
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE),
            Motes::new(DELEGATOR_1_STAKE),
        );
        let orphaned_delegator = GenesisAccount::delegator(
            missing_validator,
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE),
            Motes::new(DELEGATOR_1_STAKE),
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
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE),
            Motes::new(DELEGATOR_1_STAKE),
        );
        let duplicated_delegator_1 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE),
            Motes::new(DELEGATOR_1_STAKE),
        );
        let duplicated_delegator_2 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_2.clone(),
            Motes::new(DELEGATOR_2_BALANCE),
            Motes::new(DELEGATOR_2_STAKE),
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
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND),
                DelegationRate::MAX,
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
            Motes::new(ACCOUNT_1_BALANCE),
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
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(Motes::new(ACCOUNT_1_BOND), 80)),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE),
            Motes::new(DELEGATOR_1_STAKE),
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
    let key_map = bids.delegator_map();
    let validator_keys = key_map.keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        validator_keys,
        BTreeSet::from_iter(vec![ACCOUNT_1_PK.clone(), ACCOUNT_2_PK.clone()])
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
        delegator.delegator_kind(),
        &DelegatorKind::PublicKey(DELEGATOR_1.clone()),
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
            Motes::new(VALIDATOR_1_STAKE),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            VALIDATOR_1.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            Motes::new(DELEGATOR_1_STAKE),
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
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE - 1),
        },
    )
    .build();

    builder.exec(partial_undelegate).commit();
    let error = builder
        .get_last_exec_result()
        .expect("should have last exec result")
        .error()
        .cloned()
        .expect("should have error");

    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == auction::Error::DelegatorFundsLocked as u8
    ));
}

#[ignore]
#[test]
fn should_not_fully_undelegate_uninitialized_vesting_schedule() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let validator_1 = GenesisAccount::account(
            VALIDATOR_1.clone(),
            Motes::new(VALIDATOR_1_STAKE),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            VALIDATOR_1.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            Motes::new(DELEGATOR_1_STAKE),
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
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE),
        },
    )
    .build();

    builder.exec(full_undelegate).commit();
    let error = builder
        .get_last_exec_result()
        .expect("should have last exec result")
        .error()
        .cloned()
        .expect("should have error");

    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == auction::Error::DelegatorFundsLocked as u8
    ));
}

#[ignore]
#[test]
fn should_not_undelegate_vfta_holder_stake() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let validator_1 = GenesisAccount::account(
            VALIDATOR_1.clone(),
            Motes::new(VALIDATOR_1_STAKE),
            Some(GenesisValidator::new(
                Motes::new(VALIDATOR_1_STAKE),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            VALIDATOR_1.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
            Motes::new(DELEGATOR_1_STAKE),
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
            DEFAULT_GENESIS_CONFIG_HASH,
            DEFAULT_PROTOCOL_VERSION,
            exec_config,
            DEFAULT_CHAINSPEC_REGISTRY.clone(),
        )
    };
    let chainspec = ChainspecConfig::default()
        .with_vesting_schedule_period_millis(CASPER_VESTING_SCHEDULE_PERIOD_MILLIS);

    let mut builder = LmdbWasmTestBuilder::new_temporary_with_config(chainspec);

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
            .delegator_by_kind(&VALIDATOR_1, &DelegatorKind::PublicKey(DELEGATOR_1.clone()))
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
            .delegator_by_kind(&VALIDATOR_1, &DelegatorKind::PublicKey(DELEGATOR_1.clone()))
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
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
            ARG_AMOUNT => U512::from(DELEGATOR_1_STAKE - 1),
        },
    )
    .build();
    builder.exec(partial_unbond).commit();
    let error = builder
        .get_last_exec_result()
        .expect("should have last exec result")
        .error()
        .cloned()
        .expect("should have error");

    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == auction::Error::DelegatorFundsLocked as u8
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
                ARG_VALIDATOR => ACCOUNT_1_PK.clone(),
                ARG_DELEGATOR => DELEGATOR_1.clone(),
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
                ARG_VALIDATOR => ACCOUNT_1_PK.clone(),
                ARG_DELEGATOR => DELEGATOR_1.clone(),
                ARG_AMOUNT => U512::from(amount),
            },
        )
        .build();

        builder.exec(full_undelegate).commit();

        let error = builder
            .get_last_exec_result()
            .expect("should have last exec result")
            .error()
            .cloned()
            .expect("should have error");

        assert!(
            matches!(
                error,
                Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
                if auction_error == auction::Error::DelegatorFundsLocked as u8
            ),
            "{:?}",
            error
        );
    };

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE),
            Motes::new(DELEGATOR_VFTA_STAKE),
        );
        tmp.push(account_1);
        tmp.push(delegator_1);
        tmp
    };

    let run_genesis_request = {
        let genesis_config = GenesisConfigBuilder::default()
            .with_accounts(accounts)
            .with_locked_funds_period_millis(CASPER_LOCKED_FUNDS_PERIOD_MILLIS)
            .build();

        GenesisRequest::new(
            DEFAULT_GENESIS_CONFIG_HASH,
            DEFAULT_PROTOCOL_VERSION,
            genesis_config,
            DEFAULT_CHAINSPEC_REGISTRY.clone(),
        )
    };

    let chainspec = ChainspecConfig::default()
        .with_vesting_schedule_period_millis(CASPER_VESTING_SCHEDULE_PERIOD_MILLIS)
        .with_minimum_delegation_amount(NEW_MINIMUM_DELEGATION_AMOUNT);

    let mut builder = LmdbWasmTestBuilder::new_temporary_with_config(chainspec);

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
            .delegator_by_kind(
                &ACCOUNT_1_PK,
                &DelegatorKind::PublicKey(DELEGATOR_1.clone()),
            )
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
            .delegator_by_kind(
                &ACCOUNT_1_PK,
                &DelegatorKind::PublicKey(DELEGATOR_1.clone()),
            )
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

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(Motes::new(ACCOUNT_1_BOND), 80)),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND),
                DelegationRate::zero(),
            )),
        );
        let delegator_1 = GenesisAccount::delegator(
            ACCOUNT_1_PK.clone(),
            DELEGATOR_1.clone(),
            Motes::new(DELEGATOR_1_BALANCE),
            Motes::zero(),
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
                Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE),
                Some(GenesisValidator::new(Motes::new(ACCOUNT_1_BOND), 80)),
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

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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

    let unbond_kind = UnbondKind::DelegatedPublicKey(BID_ACCOUNT_1_PK.clone());
    let after_redelegation = builder
        .get_unbonds()
        .get(&unbond_kind)
        .expect("must have unbond")
        .first()
        .expect("must have an entry for the unbond")
        .eras()
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
    assert_eq!(bids.len(), 3);

    assert!(
        bids.delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK)
            .is_none(),
        "fully unbonded"
    );

    let delegators = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_2_PK)
        .expect("should have delegators");
    assert_eq!(delegators.len(), 1);
    let delegator = bids
        .delegator_by_kind(
            &NON_FOUNDER_VALIDATOR_2_PK,
            &DelegatorKind::PublicKey(BID_ACCOUNT_1_PK.clone()),
        )
        .expect("should have delegator");
    let redelegated_amount_1 = delegator.staked_amount();
    assert_eq!(
        redelegated_amount_1,
        U512::from(DELEGATE_AMOUNT_1),
        "expected full unbond"
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

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    builder.advance_eras_by_default_auction_delay();

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

    builder.exec(invalid_redelegate_request).expect_failure();

    let error = builder.get_error().expect("expected error");
    let str = format!("{}", error);
    assert!(
        str.starts_with("ApiError::AuctionError(RedelegationValidatorNotFound)"),
        "expected RedelegationValidatorNotFound"
    )
}

#[ignore]
#[test]
fn should_enforce_minimum_delegation_amount() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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
        assert!(
            builder.step(step_request).is_success(),
            "must execute step request"
        );
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
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::DelegationAmountTooSmall as u8));
}

#[ignore]
#[test]
fn should_allow_delegations_with_minimal_floor_amount() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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

        assert!(
            builder.step(step_request).is_success(),
            "must execute step request"
        );
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
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
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
    let chainspec = ChainspecConfig::default().with_max_delegators_per_validator(2u32);

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new_with_config(data_dir.path(), chainspec);

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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

        assert!(
            builder.step(step_request).is_success(),
            "must execute step request"
        );
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
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ExceededDelegatorSizeLimit as u8));

    let delegator_2_staked_amount = {
        let bids = builder.get_bids();
        let delegator = bids
            .delegator_by_kind(
                &NON_FOUNDER_VALIDATOR_1_PK,
                &DelegatorKind::PublicKey(BID_ACCOUNT_2_PK.clone()),
            )
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
        .collect::<Vec<&auction::DelegatorBid>>()
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

    let chainspec = ChainspecConfig::default().with_max_delegators_per_validator(1u32);

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new_with_config(data_dir.path(), chainspec);

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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

    let unbond_kind = UnbondKind::DelegatedPublicKey(BID_ACCOUNT_1_PK.clone());
    let after_redelegation = builder
        .get_unbonds()
        .get(&unbond_kind)
        .expect("must have unbond")
        .first()
        .expect("must have at least one entry")
        .eras()
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

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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
        .delegator_by_kind(
            &NON_FOUNDER_VALIDATOR_1_PK,
            &DelegatorKind::PublicKey(BID_ACCOUNT_1_PK.clone()),
        )
        .is_none());
    assert!(bids
        .delegator_by_kind(
            &NON_FOUNDER_VALIDATOR_2_PK,
            &DelegatorKind::PublicKey(BID_ACCOUNT_1_PK.clone()),
        )
        .is_some());
}

#[ignore]
#[test]
fn should_increase_existing_delegation_when_limit_exceeded() {
    let chainspec = ChainspecConfig::default().with_max_delegators_per_validator(2);

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new_with_config(data_dir.path(), chainspec);

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

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

        assert!(
            builder.step(step_request).is_success(),
            "must execute step request"
        );
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
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
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

#[ignore]
#[test]
fn should_fail_bid_public_key_change_if_conflicting_validator_bid_exists() {
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

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    builder.advance_eras_by_default_auction_delay();

    let change_bid_public_key_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_CHANGE_BID_PUBLIC_KEY,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_NEW_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_2_PK.clone()
        },
    )
    .build();

    builder.exec(change_bid_public_key_request).expect_failure();

    let error = builder.get_error().expect("must get error");
    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ValidatorBidExistsAlready as u8));
}

#[ignore]
#[test]
fn should_change_validator_bid_public_key() {
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

    let validator_3_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *NON_FOUNDER_VALIDATOR_3_ADDR,
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

    let validator_3_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_3_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_3_PK.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_3),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_3,
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

    let post_genesis_requests = vec![
        system_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        validator_1_fund_request,
        validator_2_fund_request,
        validator_3_fund_request,
        validator_1_add_bid_request,
        validator_3_add_bid_request,
        delegator_1_validator_1_delegate_request,
        delegator_2_validator_1_delegate_request,
    ];

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    builder.advance_eras_by_default_auction_delay();

    // redelegate funds to validator 3
    // NOTE: previously, this would leave a remaining delegation to the original delegator behind
    // with less than that min delegation bid. Under the new logic if the remaining del bid amount
    // would be less than the min, it gets converted to a full unbond instead of a partial unbond
    let attempted_partial_unbond_redlegate_amount =
        U512::from(UNDELEGATE_AMOUNT_1 + DEFAULT_MINIMUM_DELEGATION_AMOUNT);
    let actual_delegated_amount = U512::from(DELEGATE_AMOUNT_1);
    assert!(
        attempted_partial_unbond_redlegate_amount < actual_delegated_amount,
        "attempted partial amount should be less than actual delegated amount"
    );
    let attempted_remaining_delegation_amount =
        actual_delegated_amount - attempted_partial_unbond_redlegate_amount;
    assert!(
        attempted_remaining_delegation_amount < DEFAULT_MINIMUM_DELEGATION_AMOUNT.into(),
        "attempted remainder should be less than minimum in this case"
    );
    let delegator_1_redelegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_REDELEGATE,
        runtime_args! {
            ARG_AMOUNT => attempted_partial_unbond_redlegate_amount,
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_1_PK.clone(),
            ARG_NEW_VALIDATOR => NON_FOUNDER_VALIDATOR_3_PK.clone()
        },
    )
    .build();

    builder
        .exec(delegator_1_redelegate_request)
        .commit()
        .expect_success();

    let bids: Vec<BidKind> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();

    assert_eq!(
        bids.len(),
        3,
        "with unbonds filtered out there should be 3 bids"
    );
    assert!(bids
        .validator_bid(&NON_FOUNDER_VALIDATOR_2_PK.clone())
        .is_none());

    // change validator 1 bid public key to validator 2 public key
    let change_bid_public_key_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_CHANGE_BID_PUBLIC_KEY,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK.clone(),
            ARG_NEW_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_2_PK.clone()
        },
    )
    .build();

    let era_id = builder.get_era();

    builder
        .exec(change_bid_public_key_request)
        .commit()
        .expect_success();

    let bids: Vec<BidKind> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();
    assert_eq!(
        bids.len(),
        4,
        "with unbonds filtered out, there should be 4 bids"
    );
    let new_validator_bid = bids
        .validator_bid(&NON_FOUNDER_VALIDATOR_2_PK.clone())
        .unwrap_or_else(|| {
            panic!(
                "should have validator bid {:?}",
                NON_FOUNDER_VALIDATOR_2_PK.clone()
            )
        });

    assert_eq!(
        builder.get_purse_balance(*new_validator_bid.bonding_purse()),
        U512::from(ADD_BID_AMOUNT_1)
    );

    let bridge = bids
        .bridge(
            &NON_FOUNDER_VALIDATOR_1_PK.clone(),
            &NON_FOUNDER_VALIDATOR_2_PK.clone(),
            &era_id,
        )
        .unwrap();
    assert_eq!(
        bridge.old_validator_public_key(),
        &NON_FOUNDER_VALIDATOR_1_PK.clone()
    );
    assert_eq!(
        bridge.new_validator_public_key(),
        &NON_FOUNDER_VALIDATOR_2_PK.clone()
    );
    assert_eq!(*bridge.era_id(), era_id);

    assert!(bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_1_PK)
        .is_none());
    let delegators = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_2_PK)
        .expect("should have delegators");
    // NOTE: previously in this test the partial redelegate would have been allowed, and thus this
    // delegator would have had delegations to the original validator (below min)
    // and the redelegated target (the redelegated amount)
    // The new logic converted it into a full unbond to avoid the remainder below min being
    // left behind.
    assert_eq!(
        delegators.len(),
        1,
        "the remaining delegator should have bridged over"
    );
    assert!(
        bids.delegator_by_kind(
            &NON_FOUNDER_VALIDATOR_2_PK,
            &DelegatorKind::PublicKey(BID_ACCOUNT_1_PK.clone()),
        )
        .is_none(),
        "the redelegated unbond should not have bridged over"
    );

    let delegator = bids
        .delegator_by_kind(
            &NON_FOUNDER_VALIDATOR_2_PK,
            &DelegatorKind::PublicKey(BID_ACCOUNT_2_PK.clone()),
        )
        .expect("should have account2 delegation");
    assert_eq!(delegator.staked_amount(), U512::from(DELEGATE_AMOUNT_2));

    // distribute rewards
    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    let total_payout = builder.base_round_reward(None, protocol_version);
    let mut rewards = BTreeMap::new();
    rewards.insert(NON_FOUNDER_VALIDATOR_1_PK.clone(), vec![total_payout]);
    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();
    let bids: Vec<BidKind> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();
    assert_eq!(
        bids.len(),
        4,
        "excluding unbonds there should now be 4 bids"
    );

    assert!(
        bids.delegator_by_kind(
            &NON_FOUNDER_VALIDATOR_2_PK,
            &DelegatorKind::PublicKey(BID_ACCOUNT_1_PK.clone()),
        )
        .is_none(),
        "should not have undelegated delegator"
    );
    let delegator = bids
        .delegator_by_kind(
            &NON_FOUNDER_VALIDATOR_2_PK,
            &DelegatorKind::PublicKey(BID_ACCOUNT_2_PK.clone()),
        )
        .expect("should have account2 delegation");
    assert!(delegator.staked_amount() > U512::from(DELEGATE_AMOUNT_2));
    let expected_reward = 12;
    assert_eq!(
        delegator.staked_amount(),
        U512::from(DELEGATE_AMOUNT_2 + expected_reward)
    );

    // advance eras until unbonds are processed
    builder.advance_eras_by(DEFAULT_UNBONDING_DELAY + 1);

    let bids = builder.get_bids();
    assert_eq!(bids.len(), 5, "with unbonds filtered there should be 5");
    let delegator = bids
        .delegator_by_kind(
            &NON_FOUNDER_VALIDATOR_3_PK,
            &DelegatorKind::PublicKey(BID_ACCOUNT_1_PK.clone()),
        )
        .expect("should have account1 delegation");

    assert_eq!(
        delegator.staked_amount(),
        U512::from(DELEGATE_AMOUNT_1 + expected_reward),
        "the fully redelegated amount plus the earned rewards"
    );
}

#[ignore]
#[test]
fn should_handle_excessively_long_bridge_record_chains() {
    let mut validators = Vec::new();
    for index in 0..21 {
        let secret_key =
            SecretKey::ed25519_from_bytes([10 + index; SecretKey::ED25519_LENGTH]).unwrap();
        let pubkey = PublicKey::from(&secret_key);
        let addr = AccountHash::from(&pubkey);
        validators.push((pubkey, addr));
    }

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

    let mut post_genesis_requests = vec![
        system_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        validator_1_fund_request,
        validator_2_fund_request,
        validator_1_add_bid_request,
        validator_2_add_bid_request,
        delegator_1_validator_1_delegate_request,
        delegator_2_validator_2_delegate_request,
    ];

    // add extra validators fund requests
    for (_pubkey, addr) in validators.iter() {
        let validator_fund_request = ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CONTRACT_TRANSFER_TO_ACCOUNT,
            runtime_args! {
                ARG_TARGET => *addr,
                ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
            },
        )
        .build();
        post_genesis_requests.push(validator_fund_request)
    }

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    builder.advance_eras_by_default_auction_delay();

    // verify delegator 2 main purse balance after delegation
    let delegator_2_main_purse = builder
        .get_entity_by_account_hash(*BID_ACCOUNT_2_ADDR)
        .expect("should have default account")
        .main_purse();
    assert_eq!(
        builder.get_purse_balance(delegator_2_main_purse),
        U512::from(TRANSFER_AMOUNT - DELEGATE_AMOUNT_2)
    );

    // redelegate funds to validator 1
    let delegator_2_redelegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_2_ADDR,
        CONTRACT_REDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_2),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_2_PK.clone(),
            ARG_DELEGATOR => BID_ACCOUNT_2_PK.clone(),
            ARG_NEW_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK.clone()
        },
    )
    .build();

    builder
        .exec(delegator_2_redelegate_request)
        .commit()
        .expect_success();

    // change validator bid public key
    let mut current_bid_public_key = NON_FOUNDER_VALIDATOR_1_PK.clone();
    let mut current_bid_addr = *NON_FOUNDER_VALIDATOR_1_ADDR;
    for (pubkey, addr) in validators.iter() {
        let change_bid_public_key_request = ExecuteRequestBuilder::standard(
            current_bid_addr,
            CONTRACT_CHANGE_BID_PUBLIC_KEY,
            runtime_args! {
                ARG_PUBLIC_KEY => current_bid_public_key.clone(),
                ARG_NEW_PUBLIC_KEY => pubkey.clone()
            },
        )
        .build();

        builder
            .exec(change_bid_public_key_request)
            .commit()
            .expect_success();

        current_bid_public_key = pubkey.clone();
        current_bid_addr = *addr;
    }

    let era_id = builder.get_era();

    let bids: Vec<_> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();
    assert_eq!(bids.len(), 25);
    let new_validator_bid = bids.validator_bid(&current_bid_public_key).unwrap();
    assert_eq!(
        builder.get_purse_balance(*new_validator_bid.bonding_purse()),
        U512::from(ADD_BID_AMOUNT_1)
    );

    // check if bridge records exist
    let mut old_public_key = NON_FOUNDER_VALIDATOR_1_PK.clone();
    for (pubkey, _addr) in validators.iter() {
        let bridge = bids
            .bridge(&old_public_key.clone(), &pubkey.clone(), &era_id)
            .unwrap();
        assert_eq!(bridge.old_validator_public_key(), &old_public_key.clone());
        assert_eq!(bridge.new_validator_public_key(), &pubkey.clone());
        assert_eq!(*bridge.era_id(), era_id);

        old_public_key = pubkey.clone();
    }

    // verify delegator bids for current validator bid
    let current_public_key = old_public_key;
    let delegators = bids
        .delegators_by_validator_public_key(&current_public_key)
        .expect("should have delegators");
    assert_eq!(delegators.len(), 1);
    let delegator = bids
        .delegator_by_kind(
            &current_public_key,
            &DelegatorKind::PublicKey(BID_ACCOUNT_1_PK.clone()),
        )
        .expect("should have account1 delegation");
    assert_eq!(delegator.staked_amount(), U512::from(DELEGATE_AMOUNT_1));
    let delegators = bids
        .delegators_by_validator_public_key(&NON_FOUNDER_VALIDATOR_2_PK)
        .expect("should have delegators");
    assert_eq!(delegators.len(), 1);
    let delegator = bids
        .delegator_by_kind(
            &NON_FOUNDER_VALIDATOR_2_PK,
            &DelegatorKind::PublicKey(BID_ACCOUNT_2_PK.clone()),
        )
        .expect("should have account2 delegation");
    assert_eq!(
        delegator.staked_amount(),
        U512::from(DELEGATE_AMOUNT_2 - UNDELEGATE_AMOUNT_2)
    );

    // distribute rewards
    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    let total_payout = builder.base_round_reward(None, protocol_version);
    let mut rewards = BTreeMap::new();
    rewards.insert(NON_FOUNDER_VALIDATOR_1_PK.clone(), vec![total_payout]);
    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let bids: Vec<_> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();
    assert_eq!(bids.len(), 25);

    let delegator = bids
        .delegator_by_kind(
            &current_public_key,
            &DelegatorKind::PublicKey(BID_ACCOUNT_1_PK.clone()),
        )
        .expect("should have account1 delegation");
    assert_eq!(delegator.staked_amount(), U512::from(DELEGATE_AMOUNT_1));

    // advance eras until unbonds are processed
    builder.advance_eras_by(DEFAULT_UNBONDING_DELAY + 1);

    let bids: Vec<_> = builder
        .get_bids()
        .into_iter()
        .filter(|bid| !bid.is_unbond())
        .collect();
    assert_eq!(bids.len(), 25);
    let delegator = bids.delegator_by_kind(
        &current_public_key,
        &DelegatorKind::PublicKey(BID_ACCOUNT_2_PK.clone()),
    );
    assert!(delegator.is_none());

    // verify that unbond was returned to main purse instead of being redelegated
    assert_eq!(
        builder.get_purse_balance(delegator_2_main_purse),
        U512::from(TRANSFER_AMOUNT - DELEGATE_AMOUNT_2 + UNDELEGATE_AMOUNT_2)
    );
}

#[ignore]
#[test]
fn credits_are_considered_when_determining_validators() {
    // In this test we have 2 genesis nodes that are validators: Node 1 and Node 2; 1 has less stake
    // than 2. We have only 2 validator slots so later we'll bid in another node with a stake
    // slightly higher than the one of node 1.
    // Under normal circumstances, since node 3 put in a higher bid, it should win the slot and kick
    // out node 1. But since we add some credits for node 1 (because it was a validator and
    // proposed blocks) it should maintain its slot.
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::account(
            ACCOUNT_1_PK.clone(),
            Motes::new(ACCOUNT_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_1_BOND),
                DelegationRate::zero(),
            )),
        );
        let account_2 = GenesisAccount::account(
            ACCOUNT_2_PK.clone(),
            Motes::new(ACCOUNT_2_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(ACCOUNT_2_BOND),
                DelegationRate::zero(),
            )),
        );
        let account_3 = GenesisAccount::account(
            BID_ACCOUNT_1_PK.clone(),
            Motes::new(BID_ACCOUNT_1_BALANCE),
            None,
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp.push(account_3);
        tmp
    };

    let mut builder = LmdbWasmTestBuilder::default();
    let config = GenesisConfigBuilder::default()
        .with_accounts(accounts)
        .with_validator_slots(2) // set up only 2 validators
        .with_auction_delay(DEFAULT_AUCTION_DELAY)
        .with_locked_funds_period_millis(DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS)
        .with_round_seigniorage_rate(DEFAULT_ROUND_SEIGNIORAGE_RATE)
        .with_unbonding_delay(DEFAULT_UNBONDING_DELAY)
        .with_genesis_timestamp_millis(DEFAULT_GENESIS_TIMESTAMP_MILLIS)
        .build();
    let run_genesis_request = GenesisRequest::new(
        DEFAULT_GENESIS_CONFIG_HASH,
        DEFAULT_PROTOCOL_VERSION,
        config,
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    );
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
    // in the genesis era both node 1 and 2 are validators.
    assert_eq!(
        genesis_validator_weights
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from_iter(vec![ACCOUNT_1_PK.clone(), ACCOUNT_2_PK.clone()])
    );

    // bid in the 3rd node with an amount just a bit more than node 1.
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK.clone(),
            ARG_AMOUNT => U512::from(ACCOUNT_1_BOND + 1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    // Add a credit for node 1 artificially (assume it has proposed a block with a transaction and
    // received credit).
    let credit_amount = U512::from(2001);
    let add_credit = HandleFeeMode::credit(
        Box::new(ACCOUNT_1_PK.clone()),
        credit_amount,
        INITIAL_ERA_ID,
    );
    builder.handle_fee(
        None,
        DEFAULT_PROTOCOL_VERSION,
        TransactionHash::from_raw([1; 32]),
        add_credit,
    );

    // run auction and compute validators for new era
    builder.run_auction(
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
        Vec::new(),
    );

    let new_validator_weights: ValidatorWeights = builder
        .get_validator_weights(new_era)
        .expect("should have first era validator weights");

    // We have only 2 slots. Node 2 should be in the set because it's the highest bidder. Node 1
    // should keep its validator slot even though it's bid is now lower than node 3. This should
    // have happened because there was a credit for node 1 added.
    assert_eq!(
        new_validator_weights.get(&ACCOUNT_2_PK),
        Some(&U512::from(ACCOUNT_2_BOND))
    );
    assert!(!new_validator_weights.contains_key(&BID_ACCOUNT_1_PK));
    let expected_amount = credit_amount.saturating_add(U512::from(ACCOUNT_1_BOND));
    assert_eq!(
        new_validator_weights.get(&ACCOUNT_1_PK),
        Some(&expected_amount)
    );
}

#[ignore]
#[test]
fn should_mark_bids_with_less_than_minimum_bid_amount_as_inactive_via_upgrade() {
    const VALIDATOR_MIN_BID_FIXTURE: &str = "validator_minimum_bid";

    const FIXTURE_MIN_BID_AMOUNT: u64 = 10_000 * 1_000_000_000;

    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(VALIDATOR_MIN_BID_FIXTURE);

    let current_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let new_protocol_version = ProtocolVersion::from_parts(
        current_protocol_version.value().major,
        current_protocol_version.value().minor + 1,
        0,
    );

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(current_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(EraId::new(1))
            .with_new_gas_hold_handling(HoldBalanceHandling::Accrued)
            .with_new_gas_hold_interval(1200u64)
            .with_validator_minimum_bid_amount(FIXTURE_MIN_BID_AMOUNT + 1)
            .build()
    };

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    let public_key = {
        let secret_key = SecretKey::ed25519_from_bytes([204; SecretKey::ED25519_LENGTH]).unwrap();
        PublicKey::from(&secret_key)
    };
    let bid = builder
        .get_validator_bid(public_key)
        .expect("must have the validator bid record");

    assert!(bid.inactive())
}
