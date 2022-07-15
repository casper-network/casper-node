use casper_engine_test_support::DEFAULT_PROPOSER_ADDR;
use casper_types::{Key, U512};

use crate::lmdb_fixture;

const DAY_MILLIS: usize = 24 * 60 * 60 * 1000;
const DAYS_IN_WEEK: usize = 7;
const WEEK_MILLIS: usize = DAYS_IN_WEEK * DAY_MILLIS;
const VESTING_SCHEDULE_LENGTH_DAYS: usize = 91;
const LOCKED_AMOUNTS_LENGTH: usize = (VESTING_SCHEDULE_LENGTH_DAYS / DAYS_IN_WEEK) + 1;

const LMDB_FIXTURE_NAME: &str = "gh_3208";

#[ignore]
#[test]
fn should_run_regression() {
    let (builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(LMDB_FIXTURE_NAME);

    let stored_value = builder
        .query(None, Key::Bid(*DEFAULT_PROPOSER_ADDR), &[])
        .unwrap();
    let bid = stored_value.as_bid().unwrap();
    assert!(bid.is_locked(7776000000));
    let vesting_schedule = bid
        .vesting_schedule()
        .expect("should have a schedule initialized already");

    let initial_stake = U512::from(1_000_000_000_000u64);

    let total_vested_amounts = {
        let mut total_vested_amounts = U512::zero();

        for i in 0..LOCKED_AMOUNTS_LENGTH {
            let timestamp =
                vesting_schedule.initial_release_timestamp_millis() + (WEEK_MILLIS * i) as u64;
            if let Some(locked_amount) = vesting_schedule.locked_amount(timestamp) {
                let current_vested_amount = initial_stake - locked_amount - total_vested_amounts;
                total_vested_amounts += current_vested_amount
            }
        }

        total_vested_amounts
    };

    assert_eq!(total_vested_amounts, initial_stake);
}

#[cfg(feature = "fixture-generators")]
mod fixture {
    use casper_engine_test_support::{
        utils, StepRequestBuilder, DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_ACCOUNT_PUBLIC_KEY,
        DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
        DEFAULT_PROPOSER_PUBLIC_KEY, DEFAULT_PROTOCOL_VERSION,
    };
    use casper_execution_engine::core::engine_state::{genesis::GenesisValidator, GenesisAccount};
    use casper_types::{Motes, U512};

    use crate::lmdb_fixture;

    use super::LMDB_FIXTURE_NAME;

    #[test]
    fn generate_gh_3208_fixture() {
        let accounts = vec![
            GenesisAccount::account(
                DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
                Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
                None,
            ),
            GenesisAccount::account(
                DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
                Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
                Some(GenesisValidator::new(
                    Motes::new(U512::from(1_000_000_000_000u64)),
                    15,
                )),
            ),
        ];

        let genesis_request = utils::create_run_genesis_request(accounts);

        lmdb_fixture::generate_fixture(LMDB_FIXTURE_NAME, genesis_request, |builder| {
            let era_end_timestamp_millis =
                DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

            // Move forward the clock and initialize vesting schedule after 91 days.
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
