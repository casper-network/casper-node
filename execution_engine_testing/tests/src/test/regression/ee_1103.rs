use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNTS,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
    MINIMUM_ACCOUNT_CREATION_BALANCE, SYSTEM_ADDR, TIMESTAMP_MILLIS_INCREMENT,
};
use casper_types::{
    account::AccountHash,
    runtime_args,
    system::auction::{DelegationRate, ARG_DELEGATOR, ARG_VALIDATOR},
    GenesisAccount, GenesisValidator, Motes, PublicKey, SecretKey, U512,
};

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_DELEGATE: &str = "delegate.wasm";
const TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;

static FAUCET: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([1; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALIDATOR_1: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([3; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALIDATOR_2: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([5; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALIDATOR_3: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([7; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_1: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([203; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_2: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([205; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_3: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([207; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});

// These values were chosen to correspond to the values in accounts.toml
// at the time of their introduction.

static FAUCET_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*FAUCET));
static FAUCET_BALANCE: Lazy<U512> = Lazy::new(|| U512::from(100_000_000_000_000_000u64));
static VALIDATOR_1_BALANCE: Lazy<U512> = Lazy::new(|| U512::from(100_000_000_000_000_000u64));
static VALIDATOR_2_BALANCE: Lazy<U512> = Lazy::new(|| U512::from(100_000_000_000_000_000u64));
static VALIDATOR_3_BALANCE: Lazy<U512> = Lazy::new(|| U512::from(100_000_000_000_000_000u64));
static VALIDATOR_1_STAKE: Lazy<U512> = Lazy::new(|| U512::from(500_000_000_000_000_000u64));
static VALIDATOR_2_STAKE: Lazy<U512> = Lazy::new(|| U512::from(400_000_000_000_000u64));
static VALIDATOR_3_STAKE: Lazy<U512> = Lazy::new(|| U512::from(300_000_000_000_000u64));
static DELEGATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_1));
static DELEGATOR_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_2));
static DELEGATOR_3_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_3));
static DELEGATOR_1_BALANCE: Lazy<U512> = Lazy::new(|| U512::from(1_000_000_000_000_000u64));
static DELEGATOR_2_BALANCE: Lazy<U512> = Lazy::new(|| U512::from(1_000_000_000_000_000u64));
static DELEGATOR_3_BALANCE: Lazy<U512> = Lazy::new(|| U512::from(1_000_000_000_000_000u64));
static DELEGATOR_1_STAKE: Lazy<U512> = Lazy::new(|| U512::from(500_000_000_000_000u64));
static DELEGATOR_2_STAKE: Lazy<U512> = Lazy::new(|| U512::from(400_000_000_000_000u64));
static DELEGATOR_3_STAKE: Lazy<U512> = Lazy::new(|| U512::from(300_000_000_000_000u64));

#[ignore]
#[test]
fn validator_scores_should_reflect_delegates() {
    let accounts = {
        let faucet = GenesisAccount::account(FAUCET.clone(), Motes::new(*FAUCET_BALANCE), None);
        let validator_1 = GenesisAccount::account(
            VALIDATOR_1.clone(),
            Motes::new(*VALIDATOR_1_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(*VALIDATOR_1_STAKE),
                DelegationRate::zero(),
            )),
        );
        let validator_2 = GenesisAccount::account(
            VALIDATOR_2.clone(),
            Motes::new(*VALIDATOR_2_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(*VALIDATOR_2_STAKE),
                DelegationRate::zero(),
            )),
        );
        let validator_3 = GenesisAccount::account(
            VALIDATOR_3.clone(),
            Motes::new(*VALIDATOR_3_BALANCE),
            Some(GenesisValidator::new(
                Motes::new(*VALIDATOR_3_STAKE),
                DelegationRate::zero(),
            )),
        );
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        tmp.push(faucet);
        tmp.push(validator_1);
        tmp.push(validator_2);
        tmp.push(validator_3);
        tmp
    };

    let system_fund_request = ExecuteRequestBuilder::standard(
        *FAUCET_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *SYSTEM_ADDR,
            ARG_AMOUNT => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    let delegator_1_fund_request = ExecuteRequestBuilder::standard(
        *FAUCET_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => *DELEGATOR_1_BALANCE
        },
    )
    .build();

    let delegator_2_fund_request = ExecuteRequestBuilder::standard(
        *FAUCET_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => *DELEGATOR_2_BALANCE
        },
    )
    .build();

    let delegator_3_fund_request = ExecuteRequestBuilder::standard(
        *FAUCET_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_3_ADDR,
            ARG_AMOUNT => *DELEGATOR_3_BALANCE
        },
    )
    .build();

    let post_genesis_requests = vec![
        system_fund_request,
        delegator_1_fund_request,
        delegator_2_fund_request,
        delegator_3_fund_request,
    ];

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = LmdbWasmTestBuilder::default();

    let run_genesis_request = utils::create_run_genesis_request(accounts);
    builder.run_genesis(run_genesis_request);

    for request in post_genesis_requests {
        builder.exec(request).commit().expect_success();
    }

    let mut era = builder.get_era();
    let auction_delay = builder.get_auction_delay();

    // Check initial weights
    {
        let era_weights = builder
            .get_validator_weights(era)
            .expect("should get validator weights");

        assert_eq!(era_weights.get(&VALIDATOR_1), Some(&*VALIDATOR_1_STAKE));
        assert_eq!(era_weights.get(&VALIDATOR_2), Some(&*VALIDATOR_2_STAKE));
        assert_eq!(era_weights.get(&VALIDATOR_3), Some(&*VALIDATOR_3_STAKE));
    }

    // Check weights after auction_delay eras
    {
        for _ in 0..=auction_delay {
            builder.run_auction(timestamp_millis, Vec::new());
            timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
        }

        era = builder.get_era();
        assert_eq!(builder.get_auction_delay(), auction_delay);

        let era_weights = builder
            .get_validator_weights(era + auction_delay)
            .expect("should get validator weights");

        assert_eq!(era_weights.get(&VALIDATOR_1), Some(&*VALIDATOR_1_STAKE));
        assert_eq!(era_weights.get(&VALIDATOR_2), Some(&*VALIDATOR_2_STAKE));
        assert_eq!(era_weights.get(&VALIDATOR_3), Some(&*VALIDATOR_3_STAKE));
    }

    // Check weights after Delegator 1 delegates to Validator 1 (and auction_delay)
    {
        let delegator_1_delegate_request = ExecuteRequestBuilder::standard(
            *DELEGATOR_1_ADDR,
            CONTRACT_DELEGATE,
            runtime_args! {
                ARG_AMOUNT => *DELEGATOR_1_STAKE,
                ARG_VALIDATOR => VALIDATOR_1.clone(),
                ARG_DELEGATOR => DELEGATOR_1.clone(),
            },
        )
        .build();

        builder
            .exec(delegator_1_delegate_request)
            .commit()
            .expect_success();

        for _ in 0..=auction_delay {
            builder.run_auction(timestamp_millis, Vec::new());
            timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
        }

        era = builder.get_era();
        assert_eq!(builder.get_auction_delay(), auction_delay);

        let era_weights = builder
            .get_validator_weights(era)
            .expect("should get validator weights");

        let validator_1_expected_stake = *VALIDATOR_1_STAKE + *DELEGATOR_1_STAKE;

        let validator_2_expected_stake = *VALIDATOR_2_STAKE;

        let validator_3_expected_stake = *VALIDATOR_3_STAKE;

        assert_eq!(
            era_weights.get(&VALIDATOR_1),
            Some(&validator_1_expected_stake)
        );
        assert_eq!(
            era_weights.get(&VALIDATOR_2),
            Some(&validator_2_expected_stake)
        );
        assert_eq!(
            era_weights.get(&VALIDATOR_3),
            Some(&validator_3_expected_stake)
        );
    }

    // Check weights after Delegator 2 delegates to Validator 1 (and auction_delay)
    {
        let delegator_2_delegate_request = ExecuteRequestBuilder::standard(
            *DELEGATOR_2_ADDR,
            CONTRACT_DELEGATE,
            runtime_args! {
                ARG_AMOUNT => *DELEGATOR_2_STAKE,
                ARG_VALIDATOR => VALIDATOR_1.clone(),
                ARG_DELEGATOR => DELEGATOR_2.clone(),
            },
        )
        .build();

        builder
            .exec(delegator_2_delegate_request)
            .commit()
            .expect_success();

        for _ in 0..=auction_delay {
            builder.run_auction(timestamp_millis, Vec::new());
            timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
        }

        era = builder.get_era();
        assert_eq!(builder.get_auction_delay(), auction_delay);

        let era_weights = builder
            .get_validator_weights(era)
            .expect("should get validator weights");

        let validator_1_expected_stake =
            *VALIDATOR_1_STAKE + *DELEGATOR_1_STAKE + *DELEGATOR_2_STAKE;

        let validator_2_expected_stake = *VALIDATOR_2_STAKE;

        let validator_3_expected_stake = *VALIDATOR_3_STAKE;

        assert_eq!(
            era_weights.get(&VALIDATOR_1),
            Some(&validator_1_expected_stake)
        );
        assert_eq!(
            era_weights.get(&VALIDATOR_2),
            Some(&validator_2_expected_stake)
        );
        assert_eq!(
            era_weights.get(&VALIDATOR_3),
            Some(&validator_3_expected_stake)
        );
    }

    // Check weights after Delegator 3 delegates to Validator 2 (and auction_delay)
    {
        let delegator_3_delegate_request = ExecuteRequestBuilder::standard(
            *DELEGATOR_3_ADDR,
            CONTRACT_DELEGATE,
            runtime_args! {
                ARG_AMOUNT => *DELEGATOR_3_STAKE,
                ARG_VALIDATOR => VALIDATOR_2.clone(),
                ARG_DELEGATOR => DELEGATOR_3.clone(),
            },
        )
        .build();

        builder
            .exec(delegator_3_delegate_request)
            .commit()
            .expect_success();

        for _ in 0..=auction_delay {
            builder.run_auction(timestamp_millis, Vec::new());
            timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
        }
        era = builder.get_era();
        assert_eq!(builder.get_auction_delay(), auction_delay);

        let era_weights = builder
            .get_validator_weights(era)
            .expect("should get validator weights");

        let validator_1_expected_stake =
            *VALIDATOR_1_STAKE + *DELEGATOR_1_STAKE + *DELEGATOR_2_STAKE;

        let validator_2_expected_stake = *VALIDATOR_2_STAKE + *DELEGATOR_3_STAKE;

        let validator_3_expected_stake = *VALIDATOR_3_STAKE;

        assert_eq!(
            era_weights.get(&VALIDATOR_1),
            Some(&validator_1_expected_stake)
        );
        assert_eq!(
            era_weights.get(&VALIDATOR_2),
            Some(&validator_2_expected_stake)
        );
        assert_eq!(
            era_weights.get(&VALIDATOR_3),
            Some(&validator_3_expected_stake)
        );
    }
}
