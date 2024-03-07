use std::{collections::BTreeSet, iter::FromIterator};

use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_INITIAL_BALANCE, MINIMUM_ACCOUNT_CREATION_BALANCE, SYSTEM_ADDR,
};
use casper_execution_engine::engine_state::engine_config::DEFAULT_MINIMUM_DELEGATION_AMOUNT;
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::auction::{
        BidsExt, DelegationRate, UnbondingPurses, ARG_DELEGATOR, ARG_VALIDATOR,
        ARG_VALIDATOR_PUBLIC_KEYS, METHOD_SLASH,
    },
    GenesisAccount, GenesisValidator, Motes, PublicKey, SecretKey, U512,
};

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_DELEGATE: &str = "delegate.wasm";
const CONTRACT_UNDELEGATE: &str = "undelegate.wasm";

const DELEGATE_AMOUNT_1: u64 = 1 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const DELEGATE_AMOUNT_2: u64 = 2 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const DELEGATE_AMOUNT_3: u64 = 3 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const UNDELEGATE_AMOUNT_1: u64 = 1;
const UNDELEGATE_AMOUNT_2: u64 = 2;
const UNDELEGATE_AMOUNT_3: u64 = 3;

const TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;

const ARG_AMOUNT: &str = "amount";

static VALIDATOR_1: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([3; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALIDATOR_2: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([4; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_1: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([5; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});

static VALIDATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALIDATOR_1));
static VALIDATOR_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALIDATOR_2));
static DELEGATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_1));

const VALIDATOR_1_STAKE: u64 = 250_000;
const VALIDATOR_2_STAKE: u64 = 350_000;

#[ignore]
#[test]
fn should_run_ee_1120_slash_delegators() {
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
                Motes::new(VALIDATOR_2_STAKE.into()),
                DelegationRate::zero(),
            )),
        );

        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(validator_1);
        tmp.push(validator_2);
        tmp
    };
    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(run_genesis_request);

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *SYSTEM_ADDR,
            "amount" => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder.exec(transfer_request_1).expect_success().commit();

    let transfer_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *DELEGATOR_1_ADDR,
            "amount" => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder.exec(transfer_request_2).expect_success().commit();

    let auction = builder.get_auction_contract_hash();

    // Validator delegates funds to other genesis validator

    let delegate_exec_request_1 = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_1),
            ARG_VALIDATOR => VALIDATOR_2.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegate_exec_request_2 = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_2),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegate_exec_request_3 = ExecuteRequestBuilder::standard(
        *VALIDATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DELEGATE_AMOUNT_3),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => VALIDATOR_2.clone(),
        },
    )
    .build();

    builder
        .exec(delegate_exec_request_1)
        .expect_success()
        .commit();

    builder
        .exec(delegate_exec_request_2)
        .expect_success()
        .commit();

    builder
        .exec(delegate_exec_request_3)
        .expect_success()
        .commit();

    // Ensure that initial bid entries exist for validator 1 and validator 2
    let initial_bids = builder.get_bids();
    let key_map = initial_bids.public_key_map();
    let initial_bids_keys = key_map.keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        initial_bids_keys,
        BTreeSet::from_iter(vec![VALIDATOR_2.clone(), VALIDATOR_1.clone()])
    );

    let initial_unbond_purses: UnbondingPurses = builder.get_unbonds();
    assert_eq!(initial_unbond_purses.len(), 0);

    // DELEGATOR_1 partially unbonds from VALIDATOR_1
    let undelegate_request_1 = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_1),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    // DELEGATOR_1 partially unbonds from VALIDATOR_2
    let undelegate_request_2 = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_2),
            ARG_VALIDATOR => VALIDATOR_2.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    // VALIDATOR_2 partially unbonds from VALIDATOR_1
    let undelegate_request_3 = ExecuteRequestBuilder::standard(
        *VALIDATOR_2_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(UNDELEGATE_AMOUNT_3),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => VALIDATOR_2.clone(),
        },
    )
    .build();

    let expected_unbond_account_hashes = (*DELEGATOR_1_ADDR, *VALIDATOR_2_ADDR);
    builder.exec(undelegate_request_1).expect_success().commit();
    builder.exec(undelegate_request_2).expect_success().commit();
    builder.exec(undelegate_request_3).expect_success().commit();

    // Check unbonding purses before slashing
    let unbond_purses_before: UnbondingPurses = builder.get_unbonds();
    // should be an unbonding purse for each distinct undelegator
    unbond_purses_before.contains_key(&expected_unbond_account_hashes.1);
    let delegator_unbond = unbond_purses_before
        .get(&expected_unbond_account_hashes.0)
        .expect("should have entry");
    assert_eq!(
        delegator_unbond.len(),
        2,
        "this entity undelegated from 2 different validators"
    );
    let undelegate_from_v1 = delegator_unbond
        .iter()
        .find(|x| x.validator_public_key() == &*VALIDATOR_1)
        .expect("should have entry");
    assert_eq!(undelegate_from_v1.amount().as_u64(), UNDELEGATE_AMOUNT_1);
    let undelegate_from_v2 = delegator_unbond
        .iter()
        .find(|x| x.validator_public_key() == &*VALIDATOR_2)
        .expect("should have entry");
    assert_eq!(undelegate_from_v2.amount().as_u64(), UNDELEGATE_AMOUNT_2);

    let dual_role_unbond = unbond_purses_before
        .get(&expected_unbond_account_hashes.1)
        .expect("should have entry for entity that is both a validator and has also delegated to a different validator then unbonded from that other validator");
    assert_eq!(
        dual_role_unbond.len(),
        1,
        "this entity undelegated from 1 validator"
    );
    let undelegate_from_v1 = dual_role_unbond
        .iter()
        .find(|x| x.validator_public_key() == &*VALIDATOR_1)
        .expect("should have entry");
    assert_eq!(undelegate_from_v1.amount().as_u64(), UNDELEGATE_AMOUNT_3);

    // Check bids before slashing

    let bids_before = builder.get_bids();
    /*
        There should be 5 total bids at this point:
        VALIDATOR1 and VALIDATOR2 each have a validator bid
        DELEGATOR1 is delegated to each of them for 2 more bids
        VALIDATOR2 is also delegated to VALIDATOR1 for 1 more bid
    */
    assert_eq!(bids_before.len(), 5);
    let bids_before_keys = bids_before
        .public_key_map()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();

    assert_eq!(
        bids_before_keys, initial_bids_keys,
        "prior to taking action, keys should match initial keys"
    );

    let slash_request_1 = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        auction,
        METHOD_SLASH,
        runtime_args! {
            ARG_VALIDATOR_PUBLIC_KEYS => vec![VALIDATOR_2.clone()]
        },
    )
    .build();

    builder.exec(slash_request_1).expect_success().commit();

    // Compare bids after slashing validator 2
    let bids_after = builder.get_bids();
    assert_ne!(bids_before, bids_after);
    /*
        there should be 3 total bids at this point:
        VALIDATOR1 was not slashed, and their bid remains
        DELEGATOR1 is still delegated to VALIDATOR1 and their bid remains
        VALIDATOR2's validator bid was slashed (and removed), but they are
            also delegated to VALIDATOR1 and that delegation bid remains
    */
    assert_eq!(bids_after.len(), 3);
    assert!(bids_after.validator_bid(&VALIDATOR_2).is_none());

    let validator_1_bid = bids_after
        .validator_bid(&VALIDATOR_1)
        .expect("should have validator1 bid");
    let delegators = bids_after
        .delegators_by_validator_public_key(validator_1_bid.validator_public_key())
        .expect("should have delegators");
    assert_eq!(delegators.len(), 2);

    bids_after.delegator_by_public_keys(&VALIDATOR_1, &VALIDATOR_2).expect("the delegation record from VALIDATOR2 should exist on VALIDATOR1, in this particular and unusual edge case");
    bids_after
        .delegator_by_public_keys(&VALIDATOR_1, &DELEGATOR_1)
        .expect("the delegation record from DELEGATOR_1 should exist on VALIDATOR1");

    let unbond_purses_after: UnbondingPurses = builder.get_unbonds();
    assert_ne!(unbond_purses_before, unbond_purses_after);
    assert!(unbond_purses_after.get(&VALIDATOR_1_ADDR).is_none());
    assert!(unbond_purses_after.get(&DELEGATOR_1_ADDR).is_some());
    assert!(unbond_purses_after.get(&VALIDATOR_2_ADDR).is_some());

    // slash validator 1 to clear remaining bids and unbonding purses
    let slash_request_2 = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        auction,
        METHOD_SLASH,
        runtime_args! {
            ARG_VALIDATOR_PUBLIC_KEYS => vec![VALIDATOR_1.clone()]
        },
    )
    .build();

    builder.exec(slash_request_2).expect_success().commit();

    let bids_after = builder.get_bids();
    assert_eq!(
        bids_after.len(),
        0,
        "we slashed everybody so there should be no bids remaining"
    );

    let unbond_purses_after: UnbondingPurses = builder.get_unbonds();
    assert_eq!(
        unbond_purses_after.len(),
        0,
        "we slashed everybody currently unbonded so there should be no unbonds remaining"
    );
}
