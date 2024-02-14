use num_traits::Zero;
use std::collections::BTreeSet;

use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_AUCTION_DELAY, DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
    MINIMUM_ACCOUNT_CREATION_BALANCE, SYSTEM_ADDR, TIMESTAMP_MILLIS_INCREMENT,
};
use casper_types::{
    runtime_args,
    system::auction::{DelegationRate, ARG_VALIDATOR_PUBLIC_KEYS, INITIAL_ERA_ID, METHOD_SLASH},
    GenesisAccount, GenesisValidator, Motes, PublicKey, SecretKey, U512,
};
use once_cell::sync::Lazy;

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";

const ARG_AMOUNT: &str = "amount";

const TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE + 1000;

static ACCOUNT_1_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([200; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
const ACCOUNT_1_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_1_BOND: u64 = 100_000;

static ACCOUNT_2_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([202; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
const ACCOUNT_2_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_2_BOND: u64 = 200_000;

static ACCOUNT_3_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([204; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
const ACCOUNT_3_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_3_BOND: u64 = 200_000;

static ACCOUNT_4_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([206; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
const ACCOUNT_4_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;
const ACCOUNT_4_BOND: u64 = 200_000;

#[ignore]
#[test]
fn should_run_ee_1045_squash_validators() {
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
        ACCOUNT_3_PK.clone(),
        Motes::new(ACCOUNT_3_BALANCE.into()),
        Some(GenesisValidator::new(
            Motes::new(ACCOUNT_3_BOND.into()),
            DelegationRate::zero(),
        )),
    );
    let account_4 = GenesisAccount::account(
        ACCOUNT_4_PK.clone(),
        Motes::new(ACCOUNT_4_BALANCE.into()),
        Some(GenesisValidator::new(
            Motes::new(ACCOUNT_4_BOND.into()),
            DelegationRate::zero(),
        )),
    );

    let round_1_validator_squash = vec![ACCOUNT_2_PK.clone(), ACCOUNT_4_PK.clone()];
    let round_2_validator_squash = vec![ACCOUNT_1_PK.clone(), ACCOUNT_3_PK.clone()];

    let extra_accounts = vec![account_1, account_2, account_3, account_4];

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.extend(extra_accounts);
        tmp
    };

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let transfer_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let genesis_validator_weights = builder
        .get_validator_weights(INITIAL_ERA_ID)
        .expect("should have genesis validator weights");

    let mut new_era_id = INITIAL_ERA_ID + DEFAULT_AUCTION_DELAY + 1;
    assert!(builder.get_validator_weights(new_era_id).is_none());
    assert!(builder.get_validator_weights(new_era_id - 1).is_some());

    builder.exec(transfer_request_1).expect_success().commit();

    let auction_contract = builder.get_auction_contract_hash();

    let squash_request_1 = {
        let args = runtime_args! {
            ARG_VALIDATOR_PUBLIC_KEYS => round_1_validator_squash.clone(),
        };
        ExecuteRequestBuilder::contract_call_by_hash(
            *SYSTEM_ADDR,
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
            *SYSTEM_ADDR,
            auction_contract,
            METHOD_SLASH,
            args,
        )
        .build()
    };

    //
    // ROUND 1
    //
    builder.exec(squash_request_1).expect_success().commit();

    // new_era_id += 1;
    assert!(builder.get_validator_weights(new_era_id).is_none());
    assert!(builder.get_validator_weights(new_era_id - 1).is_some());

    builder.run_auction(timestamp_millis, Vec::new());
    timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;

    let post_round_1_auction_weights = builder
        .get_validator_weights(new_era_id)
        .expect("should have new era validator weights computed");

    assert_ne!(genesis_validator_weights, post_round_1_auction_weights);

    let lhs: BTreeSet<_> = genesis_validator_weights.keys().cloned().collect();
    let rhs: BTreeSet<_> = post_round_1_auction_weights.keys().cloned().collect();
    assert_eq!(
        lhs.difference(&rhs).cloned().collect::<BTreeSet<_>>(),
        round_1_validator_squash.into_iter().collect()
    );

    //
    // ROUND 2
    //
    builder.exec(squash_request_2).expect_success().commit();
    new_era_id += 1;
    assert!(builder.get_validator_weights(new_era_id).is_none());
    assert!(builder.get_validator_weights(new_era_id - 1).is_some());

    builder.run_auction(timestamp_millis, Vec::new());

    let post_round_2_auction_weights = builder
        .get_validator_weights(new_era_id)
        .expect("should have new era validator weights computed");

    assert_ne!(genesis_validator_weights, post_round_2_auction_weights);

    let lhs: BTreeSet<_> = post_round_1_auction_weights.keys().cloned().collect();
    let rhs: BTreeSet<_> = post_round_2_auction_weights.keys().cloned().collect();
    assert_eq!(
        lhs.difference(&rhs).cloned().collect::<BTreeSet<_>>(),
        round_2_validator_squash.into_iter().collect()
    );

    assert!(post_round_2_auction_weights.is_empty()); // all validators are squashed
}
