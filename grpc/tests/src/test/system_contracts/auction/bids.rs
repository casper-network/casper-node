use std::iter::FromIterator;

use auction::{
    EraId, SeigniorageRecipients, UnbondingPurses, ARG_DELEGATOR, ARG_PUBLIC_KEY, AUCTION_DELAY,
    DEFAULT_LOCKED_FUNDS_PERIOD, DEFAULT_UNBONDING_DELAY, ERA_ID_KEY, ERA_VALIDATORS_KEY,
    INITIAL_ERA_ID, SNAPSHOT_SIZE,
};
use casper_engine_test_support::{
    internal::{
        utils, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
};
use casper_execution_engine::{core::engine_state::genesis::GenesisAccount, shared::motes::Motes};
use casper_types::{
    self,
    account::AccountHash,
    auction::{
        self, Bids, DelegationRate, Delegators, EraValidators, ARG_AMOUNT, ARG_DELEGATION_RATE,
        ARG_VALIDATOR, BIDS_KEY,
    },
    runtime_args, PublicKey, RuntimeArgs, U512,
};

const ARG_ENTRY_POINT: &str = "entry_point";

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_AUCTION_BIDS: &str = "auction_bids.wasm";
const TRANSFER_AMOUNT: u64 = 250_000_000 + 1000;
const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);
const NON_FOUNDER_VALIDATOR_1: PublicKey = PublicKey::Ed25519([3; 32]);
const NON_FOUNDER_VALIDATOR_1_ADDR: AccountHash = AccountHash::new([4; 32]);

const ADD_BID_AMOUNT_1: u64 = 95_000;
const ADD_BID_DELEGATION_RATE_1: DelegationRate = 125;
const BID_AMOUNT_2: u64 = 5_000;
const ADD_BID_DELEGATION_RATE_2: DelegationRate = 126;
const WITHDRAW_BID_AMOUNT_2: u64 = 15_000;

const ARG_ADD_BID: &str = "add_bid";
const ARG_WITHDRAW_BID: &str = "withdraw_bid";
const ARG_DELEGATE: &str = "delegate";
const ARG_UNDELEGATE: &str = "undelegate";
const ARG_RUN_AUCTION: &str = "run_auction";
const ARG_READ_SEIGNIORAGE_RECIPIENTS: &str = "read_seigniorage_recipients";

const DELEGATE_AMOUNT_1: u64 = 125_000;
const DELEGATE_AMOUNT_2: u64 = 15_000;
const UNDELEGATE_AMOUNT_1: u64 = 35_000;

const ACCOUNT_1_PK: PublicKey = PublicKey::Ed25519([200; 32]);
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([201; 32]);
const ACCOUNT_1_BALANCE: u64 = 10_000_000;
const ACCOUNT_1_BOND: u64 = 100_000;

const ACCOUNT_2_PK: PublicKey = PublicKey::Ed25519([202; 32]);
const ACCOUNT_2_ADDR: AccountHash = AccountHash::new([203; 32]);
const ACCOUNT_2_BALANCE: u64 = 25_000_000;
const ACCOUNT_2_BOND: u64 = 200_000;

const BID_ACCOUNT_PK: PublicKey = PublicKey::Ed25519([204; 32]);
const BID_ACCOUNT_ADDR: AccountHash = AccountHash::new([205; 32]);

#[ignore]
#[test]
fn should_run_add_bid() {
    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let auction_hash = builder.get_auction_contract_hash();

    //
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_PK,
            ARG_ENTRY_POINT => ARG_ADD_BID,
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.exec(exec_request_1).commit().expect_success();

    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);

    assert_eq!(bids.len(), 1);

    let active_bid = bids.get(&BID_ACCOUNT_PK.clone()).unwrap();
    assert_eq!(
        builder.get_purse_balance(active_bid.bonding_purse),
        U512::from(ADD_BID_AMOUNT_1)
    );
    assert_eq!(active_bid.delegation_rate, ADD_BID_DELEGATION_RATE_1);
    assert_eq!(active_bid.funds_locked, None);

    // 2nd bid top-up
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_PK,
            ARG_ENTRY_POINT => ARG_ADD_BID,
            ARG_AMOUNT => U512::from(BID_AMOUNT_2),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_2,
        },
    )
    .build();

    builder.exec(exec_request_2).commit().expect_success();

    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);

    assert_eq!(bids.len(), 1);

    let active_bid = bids.get(&BID_ACCOUNT_PK.clone()).unwrap();
    assert_eq!(
        builder.get_purse_balance(active_bid.bonding_purse),
        U512::from(ADD_BID_AMOUNT_1 + BID_AMOUNT_2)
    );
    assert_eq!(active_bid.delegation_rate, ADD_BID_DELEGATION_RATE_2);

    // 3. withdraw some amount
    let exec_request_3 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => ARG_WITHDRAW_BID,
            ARG_PUBLIC_KEY => BID_ACCOUNT_PK,
            ARG_AMOUNT => U512::from(WITHDRAW_BID_AMOUNT_2),
        },
    )
    .build();
    builder.exec(exec_request_3).commit().expect_success();

    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);

    assert_eq!(bids.len(), 1);

    let active_bid = bids.get(&BID_ACCOUNT_PK.clone()).unwrap();
    assert_eq!(
        builder.get_purse_balance(active_bid.bonding_purse),
        // Since we don't pay out immediately `WITHDRAW_BID_AMOUNT_2` is locked in unbonding queue
        U512::from(ADD_BID_AMOUNT_1 + BID_AMOUNT_2)
    );
    let unbonding_purses: UnbondingPurses = builder.get_value(auction_hash, "unbonding_purses");
    let unbond_list = unbonding_purses
        .get(&BID_ACCOUNT_PK)
        .expect("should have unbond");
    assert_eq!(unbond_list.len(), 1);
    assert_eq!(unbond_list[0].origin, BID_ACCOUNT_PK);
    // `WITHDRAW_BID_AMOUNT_2` is in unbonding list
    assert_eq!(
        builder.get_purse_balance(unbond_list[0].purse),
        U512::zero(),
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
    let mut builder = InMemoryWasmTestBuilder::default();

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
            "target" => NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    // non-founding validator request
    let add_bid_request_1 = ExecuteRequestBuilder::standard(
        NON_FOUNDER_VALIDATOR_1_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_PUBLIC_KEY => NON_FOUNDER_VALIDATOR_1,
            ARG_ENTRY_POINT => ARG_ADD_BID,
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => ADD_BID_DELEGATION_RATE_1,
        },
    )
    .build();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);
    builder.exec(transfer_request_1).commit().expect_success();
    builder.exec(transfer_request_2).commit().expect_success();
    builder.exec(add_bid_request_1).commit().expect_success();

    let auction_hash = builder.get_auction_contract_hash();

    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_eq!(bids.len(), 1);
    let active_bid = bids.get(&NON_FOUNDER_VALIDATOR_1).unwrap();
    assert_eq!(
        builder.get_purse_balance(active_bid.bonding_purse),
        U512::from(ADD_BID_AMOUNT_1)
    );
    assert_eq!(active_bid.delegation_rate, ADD_BID_DELEGATION_RATE_1);

    let auction_stored_value = builder
        .query(None, auction_hash.into(), &[])
        .expect("should query auction hash");
    let _auction = auction_stored_value
        .as_contract()
        .expect("should be contract");

    //
    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => ARG_DELEGATE,
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1,
            ARG_DELEGATOR => BID_ACCOUNT_PK,
        },
    )
    .build();

    builder.exec(exec_request_1).commit().expect_success();
    let delegators: Delegators = builder.get_value(auction_hash, "delegators");
    assert_eq!(delegators.len(), 1);

    let delegated_amount_1 = delegators
        .get(&NON_FOUNDER_VALIDATOR_1.clone())
        .and_then(|map| map.get(&BID_ACCOUNT_PK.clone()))
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        delegated_amount_1,
        U512::from(DELEGATE_AMOUNT_1),
        "{:?}",
        delegators
    );

    // 2nd bid top-up
    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => ARG_DELEGATE,
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_2),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1,
            ARG_DELEGATOR => BID_ACCOUNT_PK,
        },
    )
    .build();

    builder.exec(exec_request_2).commit().expect_success();

    let delegators: Delegators = builder.get_value(auction_hash, "delegators");
    assert_eq!(delegators.len(), 1);

    let delegated_amount_2 = delegators
        .get(&NON_FOUNDER_VALIDATOR_1)
        .and_then(|map| map.get(&BID_ACCOUNT_PK.clone()))
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        delegated_amount_2,
        U512::from(DELEGATE_AMOUNT_1 + DELEGATE_AMOUNT_2),
        "{:?}",
        delegators
    );

    let exec_request_3 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => ARG_UNDELEGATE,
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_1),
            ARG_VALIDATOR => NON_FOUNDER_VALIDATOR_1,
            ARG_DELEGATOR => BID_ACCOUNT_PK,
        },
    )
    .build();
    builder.exec(exec_request_3).commit().expect_success();

    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);

    assert_eq!(bids.len(), 1);

    let delegators: Delegators = builder.get_value(auction_hash, "delegators");
    assert_eq!(delegators.len(), 1);

    let delegated_amount_3 = delegators
        .get(&NON_FOUNDER_VALIDATOR_1.clone())
        .and_then(|map| map.get(&BID_ACCOUNT_PK.clone()))
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        delegated_amount_3,
        U512::from(DELEGATE_AMOUNT_1 + DELEGATE_AMOUNT_2 - UNDELEGATE_AMOUNT_1),
        "{:?}",
        delegators
    );
}

#[ignore]
#[test]
fn should_calculate_era_validators() {
    assert_ne!(ACCOUNT_1_ADDR, ACCOUNT_2_ADDR,);
    assert_ne!(ACCOUNT_2_ADDR, BID_ACCOUNT_ADDR,);
    assert_ne!(ACCOUNT_2_ADDR, *DEFAULT_ACCOUNT_ADDR,);
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::new(
            ACCOUNT_1_PK,
            ACCOUNT_1_ADDR,
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Motes::new(ACCOUNT_1_BOND.into()),
        );
        let account_2 = GenesisAccount::new(
            ACCOUNT_2_PK,
            ACCOUNT_2_ADDR,
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Motes::new(ACCOUNT_2_BOND.into()),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = InMemoryWasmTestBuilder::default();

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
            "target" => NON_FOUNDER_VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder.run_genesis(&run_genesis_request);

    let auction_hash = builder.get_auction_contract_hash();
    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_eq!(bids.len(), 2, "founding validators {:?}", bids);

    builder.exec(transfer_request_1).commit().expect_success();
    builder.exec(transfer_request_2).commit().expect_success();

    // non-founding validator request
    let add_bid_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_PUBLIC_KEY => BID_ACCOUNT_PK,
            ARG_ENTRY_POINT => ARG_ADD_BID,
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
    assert!(era_validators.len() >= AUCTION_DELAY as usize); // definetely more than 1 element
    let (first_era, _) = era_validators.iter().min().unwrap();
    let (last_era, _) = era_validators.iter().max().unwrap();
    let expected_eras: Vec<EraId> = (*first_era..=*last_era).collect();
    assert_eq!(eras, expected_eras, "Eras {:?}", eras);

    assert!(post_era_id > 0);
    let consensus_next_era_id: EraId = AUCTION_DELAY + 1 + post_era_id;

    assert_eq!(
        era_validators.len(),
        SNAPSHOT_SIZE,
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
            .get(&BID_ACCOUNT_PK)
            .expect("should have bid account in this era"),
        &U512::from(ADD_BID_AMOUNT_1)
    );

    let era_validators_result = builder
        .get_era_validators(lookup_era_id)
        .expect("should have validator weights");
    assert_eq!(era_validators_result, *validator_weights);
}

#[ignore]
#[test]
fn should_get_first_seigniorage_recipients() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::new(
            ACCOUNT_1_PK,
            ACCOUNT_1_ADDR,
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Motes::new(ACCOUNT_1_BOND.into()),
        );
        let account_2 = GenesisAccount::new(
            ACCOUNT_2_PK,
            ACCOUNT_2_ADDR,
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
        founding_validator_1.funds_locked,
        Some(DEFAULT_LOCKED_FUNDS_PERIOD)
    );

    let founding_validator_2 = bids.get(&ACCOUNT_2_PK).expect("should have account 2 pk");
    assert_eq!(
        founding_validator_2.funds_locked,
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
    assert_eq!(era_validators.len(), SNAPSHOT_SIZE, "{:?}", era_validators); // eraindex==1 - ran once

    assert!(era_validators.contains_key(&(AUCTION_DELAY as u64 + 1)));

    let era_id = AUCTION_DELAY - 1;

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
        .get_era_validators(era_id)
        .expect("should have validator weights");
    assert_eq!(first_validator_weights, validator_weights);
}

#[ignore]
#[test]
fn should_release_founder_stake() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::new(
            ACCOUNT_1_PK,
            ACCOUNT_1_ADDR,
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Motes::new(ACCOUNT_1_BOND.into()),
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
            // This test needs a bit more tokens to run auction multiple times under `use-system-contracts` feature flag
            ARG_AMOUNT => U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE / 10)
        },
    )
    .build();

    let auction_hash = builder.get_auction_contract_hash();
    let bids: Bids = builder.get_value(auction_hash, BIDS_KEY);
    assert_eq!(bids.len(), 1);
    let (founding_validator, entry) = bids.into_iter().next().unwrap();
    assert_eq!(entry.funds_locked, Some(DEFAULT_LOCKED_FUNDS_PERIOD));
    assert_eq!(founding_validator, ACCOUNT_1_PK);

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
    assert_eq!(entry.funds_locked, None);
    assert_eq!(founding_validator, ACCOUNT_1_PK);
}

#[ignore]
#[test]
fn should_fail_to_get_era_validators() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::new(
            ACCOUNT_1_PK,
            ACCOUNT_1_ADDR,
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
        builder.get_era_validators(u64::max_value()),
        None,
        "should not have era validators for invalid era"
    );
}

#[ignore]
#[test]
fn should_use_era_validators_endpoint_for_first_era() {
    let extra_accounts = vec![GenesisAccount::new(
        ACCOUNT_1_PK,
        ACCOUNT_1_ADDR,
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
        .get_era_validators(0)
        .expect("should have validator weights for era 0");

    assert_eq!(validator_weights.len(), 1);
    assert_eq!(validator_weights[&ACCOUNT_1_PK], ACCOUNT_1_BOND.into());

    let era_validators: EraValidators =
        builder.get_value(builder.get_auction_contract_hash(), ERA_VALIDATORS_KEY);
    assert_eq!(era_validators[&0], validator_weights);
}
