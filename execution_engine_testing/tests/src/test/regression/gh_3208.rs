use once_cell::sync::Lazy;

use casper_engine_test_support::{
    utils, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, StepRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_ACCOUNT_PUBLIC_KEY,
    DEFAULT_CHAINSPEC_REGISTRY, DEFAULT_GENESIS_CONFIG_HASH, DEFAULT_GENESIS_TIMESTAMP_MILLIS,
    DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_PAYMENT, DEFAULT_PROPOSER_ADDR,
    DEFAULT_PROPOSER_PUBLIC_KEY, DEFAULT_PROTOCOL_VERSION, DEFAULT_VESTING_SCHEDULE_PERIOD_MILLIS,
};
use casper_execution_engine::{
    engine_state::{self, EngineConfigBuilder},
    execution,
};
use casper_storage::data_access_layer::GenesisRequest;
use casper_types::{
    runtime_args,
    system::{
        auction::{self, BidAddr, DelegationRate},
        standard_payment,
    },
    ApiError, GenesisAccount, GenesisConfigBuilder, GenesisValidator, Key, Motes, StoredValue,
    U512,
};

use crate::lmdb_fixture;

static DEFAULT_PROPOSER_ACCOUNT_INITIAL_STAKE: Lazy<U512> =
    Lazy::new(|| U512::from(1_000_000_000_000u64));

static ACCOUNTS_WITH_GENESIS_VALIDATORS: Lazy<Vec<GenesisAccount>> = Lazy::new(|| {
    vec![
        GenesisAccount::account(
            DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            None,
        ),
        GenesisAccount::account(
            DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            Some(GenesisValidator::new(
                Motes::new(*DEFAULT_PROPOSER_ACCOUNT_INITIAL_STAKE),
                15,
            )),
        ),
    ]
});
const DAY_MILLIS: usize = 24 * 60 * 60 * 1000;
const DAYS_IN_WEEK: usize = 7;
const WEEK_MILLIS: usize = DAYS_IN_WEEK * DAY_MILLIS;
const VESTING_SCHEDULE_LENGTH_DAYS: usize = 91;
const LOCKED_AMOUNTS_LENGTH: usize = (VESTING_SCHEDULE_LENGTH_DAYS / DAYS_IN_WEEK) + 1;

const LMDB_FIXTURE_NAME: &str = "gh_3208";

#[ignore]
#[test]
fn should_run_regression_with_already_initialized_fixed_schedule() {
    let (builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(LMDB_FIXTURE_NAME);

    let bid_key = Key::Bid(*DEFAULT_PROPOSER_ADDR);

    let stored_value = builder.query(None, bid_key, &[]).unwrap();
    if let StoredValue::Bid(bid) = stored_value {
        assert!(
            bid.is_locked_with_vesting_schedule(7776000000, DEFAULT_VESTING_SCHEDULE_PERIOD_MILLIS)
        );
        let vesting_schedule = bid
            .vesting_schedule()
            .expect("should have a schedule initialized already");

        let initial_stake = *DEFAULT_PROPOSER_ACCOUNT_INITIAL_STAKE;

        let total_vested_amounts = {
            let mut total_vested_amounts = U512::zero();

            for i in 0..LOCKED_AMOUNTS_LENGTH {
                let timestamp =
                    vesting_schedule.initial_release_timestamp_millis() + (WEEK_MILLIS * i) as u64;
                if let Some(locked_amount) = vesting_schedule.locked_amount(timestamp) {
                    let current_vested_amount =
                        initial_stake - locked_amount - total_vested_amounts;
                    total_vested_amounts += current_vested_amount
                }
            }

            total_vested_amounts
        };

        assert_eq!(total_vested_amounts, initial_stake);
    } else {
        panic!("unexpected StoredValue variant.")
    }
}

#[ignore]
#[test]
fn should_initialize_default_vesting_schedule() {
    let genesis_request =
        utils::create_run_genesis_request(ACCOUNTS_WITH_GENESIS_VALIDATORS.clone());

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(genesis_request);

    let bid_addr = BidAddr::from(*DEFAULT_PROPOSER_ADDR);
    let stored_value_before = builder
        .query(None, bid_addr.into(), &[])
        .expect("should query proposers bid");

    let bid_before = if let StoredValue::BidKind(bid) = stored_value_before {
        bid
    } else {
        panic!("Expected a bid variant in the global state");
    };

    let bid_vesting_schedule = bid_before
        .vesting_schedule()
        .expect("genesis validator should have vesting schedule");

    assert!(
        bid_vesting_schedule.locked_amounts().is_none(),
        "initial funds release is not yet processed"
    );

    let mut era_end_timestamp_millis = DEFAULT_GENESIS_TIMESTAMP_MILLIS;

    era_end_timestamp_millis += DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    builder
        .step(
            StepRequestBuilder::default()
                .with_era_end_timestamp_millis(era_end_timestamp_millis)
                .with_parent_state_hash(builder.get_post_state_hash())
                .with_protocol_version(*DEFAULT_PROTOCOL_VERSION)
                .build(),
        )
        .expect("should run step to initialize a schedule");

    let stored_value_after = builder
        .query(None, bid_addr.into(), &[])
        .expect("should query proposers bid");

    let bid_after = if let StoredValue::BidKind(bid) = stored_value_after {
        bid
    } else {
        panic!("Expected a bid variant in the global state");
    };

    let bid_vesting_schedule = bid_after
        .vesting_schedule()
        .expect("genesis validator should have vesting schedule");

    assert!(
        bid_vesting_schedule.locked_amounts().is_some(),
        "initial funds release is initialized"
    );
}

#[ignore]
#[test]
fn should_immediatelly_unbond_genesis_validator_with_zero_day_vesting_schedule() {
    let vesting_schedule_period_millis = 0;

    let exec_config = {
        let accounts = ACCOUNTS_WITH_GENESIS_VALIDATORS.clone();
        GenesisConfigBuilder::new().with_accounts(accounts).build()
    };

    let genesis_request = GenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        exec_config,
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    );

    let engine_config = EngineConfigBuilder::new()
        .with_vesting_schedule_period_millis(vesting_schedule_period_millis)
        .build();

    let mut builder = LmdbWasmTestBuilder::new_temporary_with_config(engine_config);
    builder.run_genesis(genesis_request);

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        builder.get_auction_contract_hash(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => U512::from(1u64),
            auction::ARG_DELEGATION_RATE => 10 as DelegationRate,
        },
    )
    .build();

    builder.exec(add_bid_request).expect_success().commit();

    let withdraw_bid_request_1 = {
        let sender = *DEFAULT_PROPOSER_ADDR;
        let contract_hash = builder.get_auction_contract_hash();
        let entry_point = auction::METHOD_WITHDRAW_BID;
        let payment_args = runtime_args! { standard_payment::ARG_AMOUNT => *DEFAULT_PAYMENT, };
        let session_args = runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => *DEFAULT_PROPOSER_ACCOUNT_INITIAL_STAKE,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, session_args)
            .with_empty_payment_bytes(payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash([58; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    let withdraw_bid_request_2 = {
        let sender = *DEFAULT_PROPOSER_ADDR;
        let contract_hash = builder.get_auction_contract_hash();
        let entry_point = auction::METHOD_WITHDRAW_BID;
        let payment_args = runtime_args! { standard_payment::ARG_AMOUNT => *DEFAULT_PAYMENT, };
        let session_args = runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => *DEFAULT_PROPOSER_ACCOUNT_INITIAL_STAKE,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, session_args)
            .with_empty_payment_bytes(payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash([59; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder
        .exec(withdraw_bid_request_1)
        .expect_failure()
        .commit();

    let error = builder.get_error().expect("should have error");
    assert!(
        matches!(error, engine_state::Error::Exec(execution::Error::Revert(ApiError::AuctionError(auction_error))) if auction_error == auction::Error::ValidatorFundsLocked as u8),
        "vesting schedule is not yet initialized"
    );

    let mut era_end_timestamp_millis = DEFAULT_GENESIS_TIMESTAMP_MILLIS;

    builder
        .step(
            StepRequestBuilder::default()
                .with_era_end_timestamp_millis(era_end_timestamp_millis)
                .with_parent_state_hash(builder.get_post_state_hash())
                .with_protocol_version(*DEFAULT_PROTOCOL_VERSION)
                .with_run_auction(true)
                .build(),
        )
        .expect("should run step to initialize a schedule");

    era_end_timestamp_millis += DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    builder
        .step(
            StepRequestBuilder::default()
                .with_era_end_timestamp_millis(era_end_timestamp_millis)
                .with_parent_state_hash(builder.get_post_state_hash())
                .with_protocol_version(*DEFAULT_PROTOCOL_VERSION)
                .with_run_auction(true)
                .build(),
        )
        .expect("should run step to initialize a schedule");

    builder
        .exec(withdraw_bid_request_2)
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn should_immediatelly_unbond_genesis_validator_with_zero_day_vesting_schedule_and_zero_day_lock() {
    let vesting_schedule_period_millis = 0;
    let locked_funds_period_millis = 0;

    let exec_config = {
        let accounts = ACCOUNTS_WITH_GENESIS_VALIDATORS.clone();
        GenesisConfigBuilder::new()
            .with_accounts(accounts)
            .with_locked_funds_period_millis(locked_funds_period_millis)
            .build()
    };

    let genesis_request = GenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        exec_config,
        DEFAULT_CHAINSPEC_REGISTRY.clone(),
    );

    let engine_config = EngineConfigBuilder::new()
        .with_vesting_schedule_period_millis(vesting_schedule_period_millis)
        .build();

    let mut builder = LmdbWasmTestBuilder::new_temporary_with_config(engine_config);
    builder.run_genesis(genesis_request);

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        builder.get_auction_contract_hash(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => U512::from(1u64),
            auction::ARG_DELEGATION_RATE => 10 as DelegationRate,
        },
    )
    .build();

    builder.exec(add_bid_request).expect_success().commit();

    let era_end_timestamp_millis = DEFAULT_GENESIS_TIMESTAMP_MILLIS;

    builder
        .step(
            StepRequestBuilder::default()
                .with_era_end_timestamp_millis(era_end_timestamp_millis)
                .with_parent_state_hash(builder.get_post_state_hash())
                .with_protocol_version(*DEFAULT_PROTOCOL_VERSION)
                .with_run_auction(true)
                .build(),
        )
        .expect("should run step to initialize a schedule");

    let withdraw_bid_request_1 = {
        let sender = *DEFAULT_PROPOSER_ADDR;
        let contract_hash = builder.get_auction_contract_hash();
        let entry_point = auction::METHOD_WITHDRAW_BID;
        let payment_args = runtime_args! { standard_payment::ARG_AMOUNT => *DEFAULT_PAYMENT, };
        let session_args = runtime_args! {
            auction::ARG_PUBLIC_KEY => DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
            auction::ARG_AMOUNT => *DEFAULT_PROPOSER_ACCOUNT_INITIAL_STAKE,
        };

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, session_args)
            .with_empty_payment_bytes(payment_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash([58; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder
        .exec(withdraw_bid_request_1)
        .expect_success()
        .commit();
}

#[cfg(feature = "fixture-generators")]
mod fixture {
    use casper_engine_test_support::{
        utils, StepRequestBuilder, DEFAULT_GENESIS_TIMESTAMP_MILLIS,
        DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_PROTOCOL_VERSION,
    };

    use crate::lmdb_fixture;

    use super::{ACCOUNTS_WITH_GENESIS_VALIDATORS, LMDB_FIXTURE_NAME};

    #[ignore]
    #[test]
    fn generate_gh_3208_fixture() {
        let genesis_request =
            utils::create_run_genesis_request(ACCOUNTS_WITH_GENESIS_VALIDATORS.clone());

        lmdb_fixture::generate_fixture(LMDB_FIXTURE_NAME, genesis_request, |builder| {
            let era_end_timestamp_millis =
                DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

            // Move forward the clock and initialize vesting schedule with 13 weeks after initial 90
            // days lock up.
            builder
                .step(
                    StepRequestBuilder::default()
                        .with_era_end_timestamp_millis(era_end_timestamp_millis)
                        .with_parent_state_hash(builder.get_post_state_hash())
                        .with_protocol_version(*DEFAULT_PROTOCOL_VERSION)
                        .build(),
                )
                .unwrap();
        })
        .unwrap();
    }
}
