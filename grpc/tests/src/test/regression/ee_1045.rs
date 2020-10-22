use std::{collections::BTreeSet, iter::FromIterator};

use casper_engine_test_support::{
    internal::{
        utils, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS,
        DEFAULT_AUCTION_DELAY, DEFAULT_INITIAL_ERA_ID,
    },
    DEFAULT_ACCOUNT_ADDR, MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_execution_engine::{core::engine_state::genesis::GenesisAccount, shared::motes::Motes};
use casper_types::{
    account::AccountHash,
    auction::{ARG_VALIDATOR_PUBLIC_KEYS, METHOD_RUN_AUCTION, METHOD_SLASH},
    runtime_args, PublicKey, RuntimeArgs, U512,
};

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_AUCTION_BIDS: &str = "auction_bids.wasm";

const ARG_AMOUNT: &str = "amount";
const ARG_ENTRY_POINT: &str = "entry_point";

const TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE + 1000;

const ACCOUNT_1_PK: PublicKey = PublicKey::Ed25519([200; 32]);
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([201; 32]);
const ACCOUNT_1_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_1_BOND: u64 = 100_000;

const ACCOUNT_2_PK: PublicKey = PublicKey::Ed25519([202; 32]);
const ACCOUNT_2_ADDR: AccountHash = AccountHash::new([203; 32]);
const ACCOUNT_2_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_2_BOND: u64 = 200_000;

const ACCOUNT_3_PK: PublicKey = PublicKey::Ed25519([150; 32]);
const ACCOUNT_3_ADDR: AccountHash = AccountHash::new([151; 32]);
const ACCOUNT_3_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_3_BOND: u64 = 200_000;

const ACCOUNT_4_PK: PublicKey = PublicKey::Ed25519([170; 32]);
const ACCOUNT_4_ADDR: AccountHash = AccountHash::new([171; 32]);
const ACCOUNT_4_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_4_BOND: u64 = 200_000;

const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);

#[ignore]
#[test]
fn should_run_ee_1045_squash_validators() {
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
    let account_3 = GenesisAccount::new(
        ACCOUNT_3_PK,
        ACCOUNT_3_ADDR,
        Motes::new(ACCOUNT_3_BALANCE.into()),
        Motes::new(ACCOUNT_3_BOND.into()),
    );
    let account_4 = GenesisAccount::new(
        ACCOUNT_4_PK,
        ACCOUNT_4_ADDR,
        Motes::new(ACCOUNT_4_BALANCE.into()),
        Motes::new(ACCOUNT_4_BOND.into()),
    );

    let round_1_validator_squash = vec![ACCOUNT_2_PK, ACCOUNT_4_PK];
    let round_2_validator_squash = vec![ACCOUNT_1_PK, ACCOUNT_3_PK];

    let extra_accounts = vec![account_1, account_2, account_3, account_4];

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.extend(extra_accounts);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let genesis_validator_weights = builder
        .get_era_validators(DEFAULT_INITIAL_ERA_ID)
        .expect("should have genesis validator weights");

    let mut new_era_id = DEFAULT_INITIAL_ERA_ID + DEFAULT_AUCTION_DELAY + 1;
    assert!(builder.get_era_validators(new_era_id).is_none());
    assert!(builder.get_era_validators(new_era_id - 1).is_some());

    builder.exec(transfer_request_1).expect_success().commit();

    let auction_contract = builder.get_auction_contract_hash();

    let squash_request_1 = {
        let args = runtime_args! {
            ARG_VALIDATOR_PUBLIC_KEYS => round_1_validator_squash.clone(),
        };
        ExecuteRequestBuilder::contract_call_by_hash(
            SYSTEM_ADDR,
            auction_contract,
            METHOD_SLASH,
            args,
        )
        .build()
    };

    let squash_request_2 = {
        let args = runtime_args! {
            ARG_VALIDATOR_PUBLIC_KEYS => round_2_validator_squash.clone(),
        };
        ExecuteRequestBuilder::contract_call_by_hash(
            SYSTEM_ADDR,
            auction_contract,
            METHOD_SLASH,
            args,
        )
        .build()
    };

    let run_auction_request_1 = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_RUN_AUCTION
        },
    )
    .build();

    let run_auction_request_2 = ExecuteRequestBuilder::standard(
        SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_RUN_AUCTION
        },
    )
    .build();

    //
    // ROUND 1
    //
    builder.exec(squash_request_1).expect_success().commit();

    // new_era_id += 1;
    assert!(builder.get_era_validators(new_era_id).is_none());
    assert!(builder.get_era_validators(new_era_id - 1).is_some());

    builder
        .exec(run_auction_request_1)
        .commit()
        .expect_success();

    let post_round_1_auction_weights = builder
        .get_era_validators(new_era_id)
        .expect("should have new era validator weights computed");

    assert_ne!(genesis_validator_weights, post_round_1_auction_weights);

    let lhs = BTreeSet::from_iter(genesis_validator_weights.keys().copied());
    let rhs = BTreeSet::from_iter(post_round_1_auction_weights.keys().copied());
    assert_eq!(
        lhs.difference(&rhs).copied().collect::<BTreeSet<_>>(),
        BTreeSet::from_iter(round_1_validator_squash)
    );

    //
    // ROUND 2
    //
    builder.exec(squash_request_2).expect_success().commit();
    new_era_id += 1;
    assert!(builder.get_era_validators(new_era_id).is_none());
    assert!(builder.get_era_validators(new_era_id - 1).is_some());

    builder
        .exec(run_auction_request_2)
        .commit()
        .expect_success();

    let post_round_2_auction_weights = builder
        .get_era_validators(new_era_id)
        .expect("should have new era validator weights computed");

    assert_ne!(genesis_validator_weights, post_round_2_auction_weights);

    let lhs = BTreeSet::from_iter(post_round_1_auction_weights.keys().copied());
    let rhs = BTreeSet::from_iter(post_round_2_auction_weights.keys().copied());
    assert_eq!(
        lhs.difference(&rhs).copied().collect::<BTreeSet<_>>(),
        BTreeSet::from_iter(round_2_validator_squash)
    );

    assert!(post_round_2_auction_weights.is_empty()); // all validators are squashed
}
