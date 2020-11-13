use std::{collections::BTreeSet, iter::FromIterator};

use lazy_static::lazy_static;

use casper_engine_test_support::{
    internal::{
        utils, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS,
        DEFAULT_AUCTION_DELAY, DEFAULT_LOCKED_FUNDS_PERIOD, DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::{core::engine_state::genesis::GenesisAccount, shared::motes::Motes};
use casper_types::{
    self,
    account::AccountHash,
    auction::{
        Bids, DelegationRate, EraId, EraValidators, SeigniorageRecipients, UnbondingPurses,
        ValidatorWeights, ARG_AMOUNT, ARG_DELEGATION_RATE, ARG_DELEGATOR, ARG_PUBLIC_KEY,
        ARG_UNBOND_PURSE, ARG_VALIDATOR, BIDS_KEY, DEFAULT_UNBONDING_DELAY, ERA_ID_KEY,
        ERA_VALIDATORS_KEY, INITIAL_ERA_ID, METHOD_RUN_AUCTION, UNBONDING_PURSES_KEY,
    },
    runtime_args, PublicKey, RuntimeArgs, URef, U512,
};

const ARG_ENTRY_POINT: &str = "entry_point";

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_AUCTION_BIDS: &str = "auction_bids.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";
const CONTRACT_WITHDRAW_BID: &str = "withdraw_bid.wasm";
const CONTRACT_DELEGATE: &str = "delegate.wasm";
const CONTRACT_UNDELEGATE: &str = "undelegate.wasm";
const CONTRACT_CREATE_PURSE_01: &str = "create_purse_01.wasm";

const TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE + 1000;
const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);

const ADD_BID_AMOUNT_1: u64 = 95_000;
const ADD_BID_AMOUNT_2: u64 = 47_500;
const ADD_BID_DELEGATION_RATE_1: DelegationRate = 125;
const BID_AMOUNT_2: u64 = 5_000;
const ADD_BID_DELEGATION_RATE_2: DelegationRate = 126;
const WITHDRAW_BID_AMOUNT_2: u64 = 15_000;

const ARG_RUN_AUCTION: &str = "run_auction";
const ARG_READ_SEIGNIORAGE_RECIPIENTS: &str = "read_seigniorage_recipients";

const DELEGATE_AMOUNT_1: u64 = 125_000;
const DELEGATE_AMOUNT_2: u64 = 15_000;
const UNDELEGATE_AMOUNT_1: u64 = 35_000;

const NON_FOUNDER_VALIDATOR_1_PK: PublicKey = PublicKey::Ed25519([3; 32]);
const NON_FOUNDER_VALIDATOR_2_PK: PublicKey = PublicKey::Ed25519([4; 32]);

const ACCOUNT_1_PK: PublicKey = PublicKey::Ed25519([200; 32]);
const ACCOUNT_1_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_1_BOND: u64 = 100_000;
const ACCOUNT_1_WITHDRAW_1: u64 = 55_000;
const ACCOUNT_1_WITHDRAW_2: u64 = 45_000;

const ACCOUNT_2_PK: PublicKey = PublicKey::Ed25519([202; 32]);
const ACCOUNT_2_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_2_BOND: u64 = 200_000;

const BID_ACCOUNT_1_PK: PublicKey = PublicKey::Ed25519([204; 32]);
const BID_ACCOUNT_1_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const BID_ACCOUNT_1_BOND: u64 = 0;

const BID_ACCOUNT_2_PK: PublicKey = PublicKey::Ed25519([206; 32]);
const BID_ACCOUNT_2_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const BID_ACCOUNT_2_BOND: u64 = 0;

lazy_static! {
    static ref NON_FOUNDER_VALIDATOR_1_ADDR: AccountHash = NON_FOUNDER_VALIDATOR_1_PK.into();
    static ref NON_FOUNDER_VALIDATOR_2_ADDR: AccountHash = NON_FOUNDER_VALIDATOR_2_PK.into();
    static ref ACCOUNT_1_ADDR: AccountHash = ACCOUNT_1_PK.into();
    static ref ACCOUNT_2_ADDR: AccountHash = ACCOUNT_2_PK.into();
    static ref BID_ACCOUNT_1_ADDR: AccountHash = BID_ACCOUNT_1_PK.into();
    static ref BID_ACCOUNT_2_ADDR: AccountHash = BID_ACCOUNT_2_PK.into();
}

const UNBONDING_PURSE_NAME_1: &str = "unbonding_purse_1";
const UNBONDING_PURSE_NAME_2: &str = "unbonding_purse_2";
const ARG_PURSE_NAME: &str = "purse_name";

#[ignore]
#[test]
fn should_run_add_bid() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::new(
            BID_ACCOUNT_1_PK,
            *BID_ACCOUNT_1_ADDR,
            Motes::new(BID_ACCOUNT_1_BALANCE.into()),
            Motes::new(BID_ACCOUNT_1_BOND.into()),
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK,
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(exec_request_1).commit().expect_success();

    let auction_hash = builder.get_auction_contract_hash();
    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);

    assert_eq!(bids.len(), 1);

    let active_bid = bids.get(&BID_ACCOUNT_1_PK.clone()).unwrap();
    assert_eq!(
        builder.get_purse_balance(*active_bid.bonding_purse()),
        U512::from(ADD_BID_AMOUNT_1)
    );
    assert_eq!(*active_bid.delegation_rate(), ADD_BID_DELEGATION_RATE_1);
    assert!(!active_bid.is_locked());

    // 2nd bid top-up
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK,
            ARG_AMOUNT => U512::from(BID_AMOUNT_2),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_2,
        },
    )
    .build();

    builder.exec(exec_request_2).commit().expect_success();

    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);

    assert_eq!(bids.len(), 1);

    let active_bid = bids.get(&BID_ACCOUNT_1_PK.clone()).unwrap();
    assert_eq!(
        builder.get_purse_balance(*active_bid.bonding_purse()),
        U512::from(ADD_BID_AMOUNT_1 + BID_AMOUNT_2)
    );
    assert_eq!(*active_bid.delegation_rate(), ADD_BID_DELEGATION_RATE_2);

    // 3a. create purse for unbonding purposes
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_CREATE_PURSE_01,
        runtime_args! {
            ARG_PURSE_NAME => UNBONDING_PURSE_NAME_1,
        },
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();
    let unbonding_purse = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account")
        .named_keys()
        .get(UNBONDING_PURSE_NAME_1)
        .expect("should have unbonding purse")
        .into_uref()
        .expect("unbonding purse should be an uref");

    // 3. withdraw some amount
    let exec_request_3 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK,
            ARG_AMOUNT => U512::from(WITHDRAW_BID_AMOUNT_2),
            ARG_UNBOND_PURSE => Some(unbonding_purse),
        },
    )
    .build();
    builder.exec(exec_request_3).commit().expect_success();

    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);

    assert_eq!(bids.len(), 1);

    let active_bid = bids.get(&BID_ACCOUNT_1_PK.clone()).unwrap();
    assert_eq!(
        builder.get_purse_balance(*active_bid.bonding_purse()),
        // Since we don't pay out immediately `WITHDRAW_BID_AMOUNT_2` is locked in unbonding queue
        U512::from(ADD_BID_AMOUNT_1 + BID_AMOUNT_2)
    );
    let unbonding_purses: UnbondingPurses = builder.get_value(auction_hash, "unbonding_purses");
    let unbond_list = unbonding_purses
        .get(&BID_ACCOUNT_1_PK)
        .expect("should have unbond");
    assert_eq!(unbond_list.len(), 1);
    assert_eq!(unbond_list[0].public_key, BID_ACCOUNT_1_PK);
    // `WITHDRAW_BID_AMOUNT_2` is in unbonding list

    assert_eq!(
        unbonding_purse, unbond_list[0].unbonding_purse,
        "unbonding queue should have account's unbonding purse"
    );
    assert_eq!(unbond_list[0].amount, U512::from(WITHDRAW_BID_AMOUNT_2),);

    assert_eq!(
        unbond_list[0].era_of_withdrawal,
        INITIAL_ERA_ID + DEFAULT_UNBONDING_DELAY,
    );
}

#[ignore]
#[test]
fn should_run_delegate_and_undelegate() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::new(
            BID_ACCOUNT_1_PK,
            *BID_ACCOUNT_1_ADDR,
            Motes::new(BID_ACCOUNT_1_BALANCE.into()),
            Motes::new(BID_ACCOUNT_1_BOND.into()),
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let transfer_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    // non-founding validator request
    let add_bid_request_1 = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK,
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(transfer_request_1).commit().expect_success();
    builder.exec(transfer_request_2).commit().expect_success();
    builder.exec(add_bid_request_1).commit().expect_success();

    let auction_hash = builder.get_auction_contract_hash();

    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_eq!(bids.len(), 1);
    let active_bid = bids.get(&NON_FOUNDER_VALIDATOR_1_PK).unwrap();
    assert_eq!(
        builder.get_purse_balance(*active_bid.bonding_purse()),
        U512::from(ADD_BID_AMOUNT_1)
    );
    assert_eq!(*active_bid.delegation_rate(), ADD_BID_DELEGATION_RATE_1);

    let auction_stored_value = builder
        .query(None, auction_hash.into(), &[])
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
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK,
            ARG_DELEGATOR => BID_ACCOUNT_1_PK,
        },
    )
    .build();

    builder.exec(exec_request_1).commit().expect_success();

    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_eq!(bids.len(), 1);
    let delegators = bids[&NON_FOUNDER_VALIDATOR_1_PK].delegators();
    assert_eq!(delegators.len(), 1);
    let delegated_amount_1 = *delegators[&BID_ACCOUNT_1_PK].staked_amount();
    assert_eq!(delegated_amount_1, U512::from(DELEGATE_AMOUNT_1));

    // 2nd bid top-up
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_2),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK,
            ARG_DELEGATOR => BID_ACCOUNT_1_PK,
        },
    )
    .build();

    builder.exec(exec_request_2).commit().expect_success();

    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_eq!(bids.len(), 1);
    let delegators = bids[&NON_FOUNDER_VALIDATOR_1_PK].delegators();
    assert_eq!(delegators.len(), 1);
    let delegated_amount_1 = *delegators[&BID_ACCOUNT_1_PK].staked_amount();
    assert_eq!(
        delegated_amount_1,
        U512::from(DELEGATE_AMOUNT_1 + DELEGATE_AMOUNT_2)
    );

    let exec_request_3 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK,
            ARG_DELEGATOR => BID_ACCOUNT_1_PK,
            ARG_UNBOND_PURSE => Option::<URef>::None,
        },
    )
    .build();
    builder.exec(exec_request_3).commit().expect_success();

    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_eq!(bids.len(), 1);
    let delegators = bids[&NON_FOUNDER_VALIDATOR_1_PK].delegators();
    assert_eq!(delegators.len(), 1);
    let delegated_amount_1 = *delegators[&BID_ACCOUNT_1_PK].staked_amount();
    assert_eq!(
        delegated_amount_1,
        U512::from(DELEGATE_AMOUNT_1 + DELEGATE_AMOUNT_2 - UNDELEGATE_AMOUNT_1)
    );
}

#[ignore]
#[test]
fn should_calculate_era_validators() {
    assert_ne!(*ACCOUNT_1_ADDR, *ACCOUNT_2_ADDR,);
    assert_ne!(*ACCOUNT_2_ADDR, *BID_ACCOUNT_1_ADDR,);
    assert_ne!(*ACCOUNT_2_ADDR, *DEFAULT_ACCOUNT_ADDR,);
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::new(
            ACCOUNT_1_PK,
            *ACCOUNT_1_ADDR,
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Motes::new(ACCOUNT_1_BOND.into()),
        );
        let account_2 = GenesisAccount::new(
            ACCOUNT_2_PK,
            *ACCOUNT_2_ADDR,
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Motes::new(ACCOUNT_2_BOND.into()),
        );
        let account_3 = GenesisAccount::new(
            BID_ACCOUNT_1_PK,
            *BID_ACCOUNT_1_ADDR,
            Motes::new(BID_ACCOUNT_1_BALANCE.into()),
            Motes::new(BID_ACCOUNT_1_BOND.into()),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp.push(account_3);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();
    let transfer_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let auction_hash = builder.get_auction_contract_hash();
    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_eq!(bids.len(), 2, "founding validators {:?}", bids);

    // Verify first era validators
    let first_validator_weights: ValidatorWeights = builder
        .get_validator_weights(INITIAL_ERA_ID)
        .expect("should have first era validator weights");
    assert_eq!(
        first_validator_weights
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from_iter(vec![ACCOUNT_1_PK, ACCOUNT_2_PK])
    );

    builder.exec(transfer_request_1).commit().expect_success();
    builder.exec(transfer_request_2).commit().expect_success();

    // non-founding validator request
    let add_bid_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK,
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(add_bid_request_1).commit().expect_success();

    let pre_era_id: EraId = builder.get_value(auction_hash, ERA_ID_KEY);
    assert_eq!(pre_era_id, 0);

    // non-founding validator request
    let run_auction_request_1 = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => ARG_RUN_AUCTION,
        },
    )
    .build();

    builder
        .exec(run_auction_request_1)
        .commit()
        .expect_success();

    let post_era_id: EraId = builder.get_value(auction_hash, ERA_ID_KEY);
    assert_eq!(post_era_id, 1);

    let era_validators: EraValidators = builder.get_value(auction_hash, "era_validators");

    // Check if there are no missing eras after the calculation, but we don't care about what the
    // elements are
    let eras = Vec::from_iter(era_validators.keys().copied());
    assert!(!era_validators.is_empty());
    assert!(era_validators.len() >= DEFAULT_AUCTION_DELAY as usize); // definetely more than 1 element
    let (first_era, _) = era_validators.iter().min().unwrap();
    let (last_era, _) = era_validators.iter().max().unwrap();
    let expected_eras: Vec<EraId> = (*first_era..=*last_era).collect();
    assert_eq!(eras, expected_eras, "Eras {:?}", eras);

    assert!(post_era_id > 0);
    let consensus_next_era_id: EraId = DEFAULT_AUCTION_DELAY + 1 + post_era_id;

    let snapshot_size = DEFAULT_AUCTION_DELAY as usize + 1;
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
        let account_1 = GenesisAccount::new(
            ACCOUNT_1_PK,
            *ACCOUNT_1_ADDR,
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Motes::new(ACCOUNT_1_BOND.into()),
        );
        let account_2 = GenesisAccount::new(
            ACCOUNT_2_PK,
            *ACCOUNT_2_ADDR,
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Motes::new(ACCOUNT_2_BOND.into()),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let auction_hash = builder.get_auction_contract_hash();
    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_eq!(bids.len(), 2);

    let founding_validator_1 = bids.get(&ACCOUNT_1_PK).expect("should have account 1 pk");
    assert_eq!(
        founding_validator_1.release_era(),
        Some(DEFAULT_LOCKED_FUNDS_PERIOD)
    );

    let founding_validator_2 = bids.get(&ACCOUNT_2_PK).expect("should have account 2 pk");
    assert_eq!(
        founding_validator_2.release_era(),
        Some(DEFAULT_LOCKED_FUNDS_PERIOD)
    );

    builder.exec(transfer_request_1).commit().expect_success();

    // run_auction should be executed first
    let exec_request_1 = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => ARG_RUN_AUCTION,
        },
    )
    .build();

    builder.exec(exec_request_1).commit().expect_success();

    // read seigniorage recipients
    let exec_request_2 = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => ARG_READ_SEIGNIORAGE_RECIPIENTS,
        },
    )
    .build();

    builder.exec(exec_request_2).commit().expect_success();

    let account = builder.get_account(SYSTEM_ADDR).unwrap();
    let key = account
        .named_keys()
        .get("seigniorage_recipients_result")
        .copied()
        .unwrap();
    let stored_value = builder.query(None, key, &[]).unwrap();
    let seigniorage_recipients: SeigniorageRecipients = stored_value
        .as_cl_value()
        .cloned()
        .unwrap()
        .into_t()
        .unwrap();
    assert_eq!(seigniorage_recipients.len(), 2);

    let mut era_validators: EraValidators = builder.get_value(auction_hash, "era_validators");
    let snapshot_size = DEFAULT_AUCTION_DELAY as usize + 1;

    assert_eq!(era_validators.len(), snapshot_size, "{:?}", era_validators); // eraindex==1 - ran once

    assert!(era_validators.contains_key(&(DEFAULT_AUCTION_DELAY as u64 + 1)));

    let era_id = DEFAULT_AUCTION_DELAY - 1;

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
    assert_eq!(ACCOUNT_1_WITHDRAW_1 + ACCOUNT_1_WITHDRAW_2, ACCOUNT_1_BOND);
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::new(
            ACCOUNT_1_PK,
            *ACCOUNT_1_ADDR,
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Motes::new(ACCOUNT_1_BOND.into()),
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let create_purse_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_CREATE_PURSE_01,
        runtime_args! {
            ARG_PURSE_NAME => UNBONDING_PURSE_NAME_1,
        },
    )
    .build();
    let create_purse_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_CREATE_PURSE_01,
        runtime_args! {
            ARG_PURSE_NAME => UNBONDING_PURSE_NAME_2,
        },
    )
    .build();

    builder.exec(create_purse_1).expect_success().commit();
    builder.exec(create_purse_2).expect_success().commit();

    let unbonding_purse_1 = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account")
        .named_keys()
        .get(UNBONDING_PURSE_NAME_1)
        .expect("should have unbonding purse")
        .into_uref()
        .expect("unbonding purse should be an uref");
    let unbonding_purse_2 = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account")
        .named_keys()
        .get(UNBONDING_PURSE_NAME_2)
        .expect("should have unbonding purse")
        .into_uref()
        .expect("unbonding purse should be an uref");

    let auction = builder.get_auction_contract_hash();

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            // This test needs a bit more tokens to run auction multiple times under `use-system-contracts` feature flag
            ARG_AMOUNT => U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE / 10)
        },
    )
    .build();

    let auction_hash = builder.get_auction_contract_hash();
    let genesis_bids: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_eq!(genesis_bids.len(), 1);
    let entry = genesis_bids.get(&ACCOUNT_1_PK).unwrap();
    assert_eq!(entry.release_era(), Some(DEFAULT_LOCKED_FUNDS_PERIOD));

    builder.exec(transfer_request_1).commit().expect_success();

    for _ in 0..=DEFAULT_LOCKED_FUNDS_PERIOD {
        let run_auction_request = ExecuteRequestBuilder::standard(
            SYSTEM_ADDR,
            CONTRACT_AUCTION_BIDS,
            runtime_args! {
                ARG_ENTRY_POINT => ARG_RUN_AUCTION,
            },
        )
        .build();
        builder.exec(run_auction_request).commit().expect_success();
    }

    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_eq!(bids.len(), 1);
    let (founding_validator, entry) = bids.into_iter().next().unwrap();
    assert_eq!(entry.release_era(), None);
    assert_eq!(
        builder.get_purse_balance(*entry.bonding_purse()),
        ACCOUNT_1_BOND.into()
    );
    assert_eq!(founding_validator, ACCOUNT_1_PK);

    // withdraw unlocked funds with partial amounts
    let withdraw_bid_request_1 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => ACCOUNT_1_PK,
            ARG_AMOUNT => U512::from(ACCOUNT_1_WITHDRAW_1),
            ARG_UNBOND_PURSE => Some(unbonding_purse_1),
        },
    )
    .build();
    let withdraw_bid_request_2 = ExecuteRequestBuilder::standard(
        *ACCOUNT_1_ADDR,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => ACCOUNT_1_PK,
            ARG_AMOUNT => U512::from(ACCOUNT_1_WITHDRAW_2),
            ARG_UNBOND_PURSE => Some(unbonding_purse_2),
        },
    )
    .build();

    let pre_unbond_purses: UnbondingPurses = builder.get_value(auction, UNBONDING_PURSES_KEY);
    assert_eq!(pre_unbond_purses.len(), 0);

    //
    // founder does withdraw request 1 in INITIAL_ERA_ID
    //

    builder
        .exec(withdraw_bid_request_1)
        .commit()
        .expect_success();

    let post_bids_1: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_ne!(post_bids_1, genesis_bids);
    assert_eq!(
        *post_bids_1[&ACCOUNT_1_PK].staked_amount(),
        U512::from(ACCOUNT_1_BOND - ACCOUNT_1_WITHDRAW_1)
    );

    // run auction to increase ERA ID
    let run_auction_request_1 = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => ARG_RUN_AUCTION,
        },
    )
    .build();

    builder
        .exec(run_auction_request_1)
        .commit()
        .expect_success();

    //
    // founder does withdraw request 2 in INITIAL_ERA_ID + 1
    //
    builder
        .exec(withdraw_bid_request_2)
        .commit()
        .expect_success();

    let post_bids_2: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_ne!(post_bids_2, genesis_bids);
    assert_ne!(post_bids_2, post_bids_1);
    assert!(post_bids_2.is_empty());

    // original bonding purse is not updated (yet)
    assert_eq!(
        builder.get_purse_balance(*entry.bonding_purse()),
        ACCOUNT_1_BOND.into()
    );

    let pre_unbond_purses: UnbondingPurses = builder.get_value(auction, UNBONDING_PURSES_KEY);
    assert_eq!(pre_unbond_purses.len(), 1);
    let pre_unbond_list = pre_unbond_purses
        .get(&ACCOUNT_1_PK)
        .expect("should have unbond");
    assert_eq!(pre_unbond_list.len(), 2);
    assert_eq!(pre_unbond_list[0].public_key, ACCOUNT_1_PK);
    assert_eq!(pre_unbond_list[0].amount, ACCOUNT_1_WITHDRAW_1.into());
    assert_eq!(pre_unbond_list[1].public_key, ACCOUNT_1_PK);
    assert_eq!(pre_unbond_list[1].amount, ACCOUNT_1_WITHDRAW_2.into());

    // Funds are not transferred yet from the original bonding purse
    assert_eq!(
        builder.get_purse_balance(pre_unbond_list[0].unbonding_purse),
        U512::zero(),
    );
    assert_eq!(
        builder.get_purse_balance(pre_unbond_list[1].unbonding_purse),
        U512::zero(),
    );
    // check that bids are updated for given validator

    for _ in 0..DEFAULT_UNBONDING_DELAY {
        let run_auction_request_1 = ExecuteRequestBuilder::standard(
            SYSTEM_ADDR,
            CONTRACT_AUCTION_BIDS,
            runtime_args! {
                ARG_ENTRY_POINT => ARG_RUN_AUCTION,
            },
        )
        .build();

        builder
            .exec(run_auction_request_1)
            .commit()
            .expect_success();
    }

    // Should pay out withdraw_bid request from INITIAL_ERA_ID

    //
    // Funds are transferred from the original bonding purse to the unbonding purses
    //
    assert_eq!(
        builder.get_purse_balance(pre_unbond_list[0].unbonding_purse), // still valid
        ACCOUNT_1_WITHDRAW_1.into(),
    );
    assert_eq!(
        builder.get_purse_balance(pre_unbond_list[1].unbonding_purse), // still valid
        U512::zero(),
    );

    let exec_request_4 = ExecuteRequestBuilder::contract_call_by_hash(
        SYSTEM_ADDR,
        auction,
        METHOD_RUN_AUCTION,
        runtime_args! {},
    )
    .build();

    //
    // Pays out withdraw_bid request that happened in INITIAL_ERA_ID + 1
    //
    builder.exec(exec_request_4).expect_success().commit();

    assert_eq!(
        builder.get_purse_balance(pre_unbond_list[0].unbonding_purse), // still valid ref
        ACCOUNT_1_WITHDRAW_1.into(),
    );
    assert_eq!(
        builder.get_purse_balance(pre_unbond_list[1].unbonding_purse), // still valid ref
        ACCOUNT_1_WITHDRAW_2.into(),
    );

    let post_unbond_purses: UnbondingPurses = builder.get_value(auction, UNBONDING_PURSES_KEY);
    assert_eq!(post_unbond_purses.len(), 0);

    let post_bids: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_ne!(post_bids, genesis_bids);
    assert!(post_bids.is_empty());
}

#[ignore]
#[test]
fn should_fail_to_get_era_validators() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::new(
            ACCOUNT_1_PK,
            *ACCOUNT_1_ADDR,
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Motes::new(ACCOUNT_1_BOND.into()),
        );
        tmp.push(account_1);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    assert_eq!(
        builder.get_validator_weights(u64::max_value()),
        None,
        "should not have era validators for invalid era"
    );
}

#[ignore]
#[test]
fn should_use_era_validators_endpoint_for_first_era() {
    let extra_accounts = vec![GenesisAccount::new(
        ACCOUNT_1_PK,
        *ACCOUNT_1_ADDR,
        Motes::new(ACCOUNT_1_BALANCE.into()),
        Motes::new(ACCOUNT_1_BOND.into()),
    )];

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.extend(extra_accounts);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let validator_weights = builder
        .get_validator_weights(INITIAL_ERA_ID)
        .expect("should have validator weights for era 0");

    assert_eq!(validator_weights.len(), 1);
    assert_eq!(validator_weights[&ACCOUNT_1_PK], ACCOUNT_1_BOND.into());

    let era_validators: EraValidators =
        builder.get_value(builder.get_auction_contract_hash(), ERA_VALIDATORS_KEY);
    assert_eq!(era_validators[&0], validator_weights);
}

#[ignore]
#[test]
fn should_calculate_era_validators_multiple_new_bids() {
    assert_ne!(*ACCOUNT_1_ADDR, *ACCOUNT_2_ADDR,);
    assert_ne!(*ACCOUNT_2_ADDR, *BID_ACCOUNT_1_ADDR,);
    assert_ne!(*ACCOUNT_2_ADDR, *DEFAULT_ACCOUNT_ADDR,);
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::new(
            ACCOUNT_1_PK,
            *ACCOUNT_1_ADDR,
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Motes::new(ACCOUNT_1_BOND.into()),
        );
        let account_2 = GenesisAccount::new(
            ACCOUNT_2_PK,
            *ACCOUNT_2_ADDR,
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Motes::new(ACCOUNT_2_BOND.into()),
        );
        let account_3 = GenesisAccount::new(
            BID_ACCOUNT_1_PK,
            *BID_ACCOUNT_1_ADDR,
            Motes::new(BID_ACCOUNT_1_BALANCE.into()),
            Motes::new(BID_ACCOUNT_1_BOND.into()),
        );
        let account_4 = GenesisAccount::new(
            BID_ACCOUNT_2_PK,
            *BID_ACCOUNT_2_ADDR,
            Motes::new(BID_ACCOUNT_2_BALANCE.into()),
            Motes::new(BID_ACCOUNT_2_BOND.into()),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp.push(account_3);
        tmp.push(account_4);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let genesis_validator_weights = builder
        .get_validator_weights(INITIAL_ERA_ID)
        .expect("should have genesis validators for initial era");

    // new_era is the first era in the future where new era validator weights will be calculated
    let new_era = INITIAL_ERA_ID + DEFAULT_AUCTION_DELAY + 1;
    assert!(builder.get_validator_weights(new_era).is_none());
    assert_eq!(
        builder.get_validator_weights(new_era - 1).unwrap(),
        builder.get_validator_weights(INITIAL_ERA_ID).unwrap()
    );

    assert_eq!(
        genesis_validator_weights
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from_iter(vec![ACCOUNT_1_PK, ACCOUNT_2_PK])
    );

    // Fund additional accounts
    for target in &[
        SYSTEM_ADDR,
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        *NON_FOUNDER_VALIDATOR_2_ADDR,
    ] {
        let transfer_request_1 = ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CONTRACT_TRANSFER_TO_ACCOUNT,
            runtime_args! {
                "target" => *target,
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
            ARG_PUBLIC_KEY => BID_ACCOUNT_1_PK,
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();
    let add_bid_request_2 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_2_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_2_PK,
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_2),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_2,
        },
    )
    .build();

    builder.exec(add_bid_request_1).commit().expect_success();
    builder.exec(add_bid_request_2).commit().expect_success();

    // run auction and compute validators for new era
    let run_auction_request_1 = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => ARG_RUN_AUCTION,
        },
    )
    .build();

    builder
        .exec(run_auction_request_1)
        .commit()
        .expect_success();

    // Verify first era validators
    let new_validator_weights: ValidatorWeights = builder
        .get_validator_weights(new_era)
        .expect("should have first era validator weights");

    // check that the new computed era has exactly the state we expect
    let lhs = new_validator_weights
        .keys()
        .copied()
        .collect::<BTreeSet<_>>();

    let rhs = BTreeSet::from_iter(vec![
        ACCOUNT_1_PK,
        ACCOUNT_2_PK,
        BID_ACCOUNT_1_PK,
        BID_ACCOUNT_2_PK,
    ]);

    assert_eq!(lhs, rhs);

    // make sure that new validators are exactly those that were part of add_bid requests
    let new_validators: BTreeSet<_> = rhs
        .difference(&genesis_validator_weights.keys().copied().collect())
        .copied()
        .collect();
    assert_eq!(
        new_validators,
        BTreeSet::from_iter(vec![BID_ACCOUNT_1_PK, BID_ACCOUNT_2_PK,])
    );
}

#[ignore]
#[test]
fn undelegated_funds_should_be_released() {
    const SYSTEM_TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;

    let system_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(SYSTEM_TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *BID_ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK,
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
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK,
            ARG_DELEGATOR => BID_ACCOUNT_1_PK,
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

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        super::run_auction(&mut builder);
    }

    let create_purse_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_CREATE_PURSE_01,
        runtime_args! {
            ARG_PURSE_NAME => UNBONDING_PURSE_NAME_1,
        },
    )
    .build();

    builder
        .exec(create_purse_request_1)
        .expect_success()
        .commit();
    let delegator_1_undelegate_purse = builder
        .get_account(*BID_ACCOUNT_1_ADDR)
        .expect("should have default account")
        .named_keys()
        .get(UNBONDING_PURSE_NAME_1)
        .expect("should have unbonding purse")
        .into_uref()
        .expect("unbonding purse should be an uref");

    let delegator_1_undelegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK,
            ARG_DELEGATOR => BID_ACCOUNT_1_PK,
            ARG_UNBOND_PURSE => Some(delegator_1_undelegate_purse),
        },
    )
    .build();

    builder
        .exec(delegator_1_undelegate_request)
        .commit()
        .expect_success();

    for _ in 0..=DEFAULT_UNBONDING_DELAY {
        let delegator_1_undelegate_purse_balance =
            builder.get_purse_balance(delegator_1_undelegate_purse);
        assert_eq!(delegator_1_undelegate_purse_balance, U512::zero());
        super::run_auction(&mut builder);
    }

    let delegator_1_undelegate_purse_balance =
        builder.get_purse_balance(delegator_1_undelegate_purse);
    assert_eq!(
        delegator_1_undelegate_purse_balance,
        U512::from(UNDELEGATE_AMOUNT_1)
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
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(SYSTEM_TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *BID_ACCOUNT_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let validator_1_add_bid_request = ExecuteRequestBuilder::standard(
        *NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1_PK,
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
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK,
            ARG_DELEGATOR => BID_ACCOUNT_1_PK,
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

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    for _ in 0..5 {
        super::run_auction(&mut builder);
    }

    let create_purse_request_1 = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_CREATE_PURSE_01,
        runtime_args! {
            ARG_PURSE_NAME => UNBONDING_PURSE_NAME_1,
        },
    )
    .build();

    builder
        .exec(create_purse_request_1)
        .expect_success()
        .commit();
    let delegator_1_undelegate_purse = builder
        .get_account(*BID_ACCOUNT_1_ADDR)
        .expect("should have default account")
        .named_keys()
        .get(UNBONDING_PURSE_NAME_1)
        .expect("should have unbonding purse")
        .into_uref()
        .expect("unbonding purse should be an uref");

    let delegator_1_undelegate_request = ExecuteRequestBuilder::standard(
        *BID_ACCOUNT_1_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1_PK,
            ARG_DELEGATOR => BID_ACCOUNT_1_PK,
            ARG_UNBOND_PURSE => Some(delegator_1_undelegate_purse),
        },
    )
    .build();

    builder
        .exec(delegator_1_undelegate_request)
        .commit()
        .expect_success();

    for _ in 0..=DEFAULT_UNBONDING_DELAY {
        let delegator_1_undelegate_purse_balance =
            builder.get_purse_balance(delegator_1_undelegate_purse);
        assert_eq!(delegator_1_undelegate_purse_balance, U512::zero());
        super::run_auction(&mut builder);
    }

    let delegator_1_undelegate_purse_balance =
        builder.get_purse_balance(delegator_1_undelegate_purse);
    assert_eq!(
        delegator_1_undelegate_purse_balance,
        U512::from(DELEGATE_AMOUNT_1)
    )
}
