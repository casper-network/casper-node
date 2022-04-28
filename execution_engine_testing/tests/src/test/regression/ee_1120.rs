use std::{collections::BTreeSet, iter::FromIterator};

use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_INITIAL_BALANCE, MINIMUM_ACCOUNT_CREATION_BALANCE, SYSTEM_ADDR,
};
use casper_execution_engine::core::engine_state::{
    engine_config::DEFAULT_MINIMUM_DELEGATION_AMOUNT,
    genesis::{GenesisAccount, GenesisValidator},
};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::auction::{
        Bids, DelegationRate, UnbondingPurses, ARG_DELEGATOR, ARG_VALIDATOR,
        ARG_VALIDATOR_PUBLIC_KEYS, METHOD_SLASH,
    },
    Motes, PublicKey, RuntimeArgs, SecretKey, U512,
};

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_DELEGATE: &str = "delegate.wasm";
const CONTRACT_UNDELEGATE: &str = "undelegate.wasm";

const DELEGATE_AMOUNT_1: u64 = 95_000 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const DELEGATE_AMOUNT_2: u64 = 42_000 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const DELEGATE_AMOUNT_3: u64 = 13_000 + DEFAULT_MINIMUM_DELEGATION_AMOUNT;
const UNDELEGATE_AMOUNT_1: u64 = 17_000;
const UNDELEGATE_AMOUNT_2: u64 = 24_500;
const UNDELEGATE_AMOUNT_3: u64 = 7_500;

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

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&run_genesis_request);

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
    let initial_bids: Bids = builder.get_bids();
    assert_eq!(
        initial_bids.keys().cloned().collect::<BTreeSet<_>>(),
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

    builder.exec(undelegate_request_1).commit().expect_success();
    builder.exec(undelegate_request_2).commit().expect_success();
    builder.exec(undelegate_request_3).commit().expect_success();

    // Check unbonding purses before slashing

    let unbond_purses_before: UnbondingPurses = builder.get_unbonds();
    assert_eq!(unbond_purses_before.len(), 2);

    let validator_1_unbond_list_before = unbond_purses_before
        .get(&VALIDATOR_1_ADDR)
        .cloned()
        .expect("should have unbond");
    assert_eq!(validator_1_unbond_list_before.len(), 2); // two entries in order: undelegate, and withdraw bid

    // Added through `undelegate_request_1`
    assert_eq!(
        validator_1_unbond_list_before[0].validator_public_key(),
        &*VALIDATOR_1
    );
    assert_eq!(
        validator_1_unbond_list_before[0].unbonder_public_key(),
        &*DELEGATOR_1
    );
    assert_eq!(
        validator_1_unbond_list_before[0].amount(),
        &U512::from(UNDELEGATE_AMOUNT_1)
    );

    // Added through `undelegate_request_3`
    assert_eq!(
        validator_1_unbond_list_before[1].validator_public_key(),
        &*VALIDATOR_1
    );
    assert_eq!(
        validator_1_unbond_list_before[1].unbonder_public_key(),
        &*VALIDATOR_2
    );
    assert_eq!(
        validator_1_unbond_list_before[1].amount(),
        &U512::from(UNDELEGATE_AMOUNT_3)
    );

    let validator_2_unbond_list = unbond_purses_before
        .get(&*VALIDATOR_2_ADDR)
        .cloned()
        .expect("should have unbond");

    assert_eq!(validator_2_unbond_list.len(), 1); // one entry: undelegate
    assert_eq!(
        validator_2_unbond_list[0].validator_public_key(),
        &*VALIDATOR_2
    );
    assert_eq!(
        validator_2_unbond_list[0].unbonder_public_key(),
        &*DELEGATOR_1
    );
    assert_eq!(
        validator_2_unbond_list[0].amount(),
        &U512::from(UNDELEGATE_AMOUNT_2),
    );

    // Check bids before slashing

    let bids_before: Bids = builder.get_bids();
    assert_eq!(
        bids_before.keys().collect::<Vec<_>>(),
        initial_bids.keys().collect::<Vec<_>>()
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
    let bids_after: Bids = builder.get_bids();
    assert_ne!(bids_before, bids_after);
    assert_eq!(bids_after.len(), 2);
    let validator_2_bid = bids_after.get(&VALIDATOR_2).unwrap();
    assert!(validator_2_bid.inactive());
    assert!(validator_2_bid.staked_amount().is_zero());

    assert!(bids_after.contains_key(&VALIDATOR_1));
    assert_eq!(bids_after[&VALIDATOR_1].delegators().len(), 2);

    // validator 2's delegation bid on validator 1 was not slashed.
    assert!(bids_after[&VALIDATOR_1]
        .delegators()
        .contains_key(&VALIDATOR_2));
    assert!(bids_after[&VALIDATOR_1]
        .delegators()
        .contains_key(&DELEGATOR_1));

    let unbond_purses_after: UnbondingPurses = builder.get_unbonds();
    assert_ne!(unbond_purses_before, unbond_purses_after);

    let validator_1_unbond_list_after = unbond_purses_after
        .get(&VALIDATOR_1_ADDR)
        .expect("should have validator 1 entry");
    assert_eq!(validator_1_unbond_list_after.len(), 2);
    assert_eq!(
        validator_1_unbond_list_after[0].unbonder_public_key(),
        &*DELEGATOR_1
    );

    // validator 2's delegation unbond from validator 1 was not slashed
    assert_eq!(
        validator_1_unbond_list_after[1].unbonder_public_key(),
        &*VALIDATOR_2
    );

    // delegator 1 had a delegation unbond slashed for validator 2's behavior.
    // delegator 1 still has an active delegation unbond from validator 2.
    assert_eq!(
        validator_1_unbond_list_after,
        &validator_1_unbond_list_before
    );

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

    let bids_after: Bids = builder.get_bids();
    assert_eq!(bids_after.len(), 2);
    let validator_1_bid = bids_after.get(&VALIDATOR_1).unwrap();
    assert!(validator_1_bid.inactive());
    assert!(validator_1_bid.staked_amount().is_zero());

    let unbond_purses_after: UnbondingPurses = builder.get_unbonds();
    assert!(unbond_purses_after
        .get(&VALIDATOR_1_ADDR)
        .unwrap()
        .is_empty());
    assert!(unbond_purses_after
        .get(&VALIDATOR_2_ADDR)
        .unwrap()
        .is_empty());
}
