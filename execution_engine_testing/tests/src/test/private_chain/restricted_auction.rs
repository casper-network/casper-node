use casper_engine_test_support::{
    StepRequestBuilder, DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
    DEFAULT_PROTOCOL_VERSION, TIMESTAMP_MILLIS_INCREMENT,
};
use casper_execution_engine::core::engine_state::RewardItem;
use casper_types::{system::auction::SeigniorageAllocation, Key, U512};

use crate::test::private_chain::{PRIVATE_CHAIN_GENESIS_VALIDATORS, VALIDATOR_1_PUBLIC_KEY};

#[ignore]
#[test]
fn should_not_distribute_rewards_but_compute_next_set() {
    const VALIDATOR_1_REWARD_FACTOR: u64 = 0;

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = super::private_chain_setup();

    // initial token supply
    let initial_supply = builder.total_supply(None);

    for _ in 0..3 {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(*DEFAULT_PROTOCOL_VERSION)
            .with_next_era_id(builder.get_era().successor())
            .with_era_end_timestamp_millis(timestamp_millis)
            .with_run_auction(true)
            .with_reward_item(RewardItem::new(
                VALIDATOR_1_PUBLIC_KEY.clone(),
                VALIDATOR_1_REWARD_FACTOR,
            ))
            .build();
        builder.step(step_request).expect("should execute step");
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let last_trusted_era = builder.get_era();

    let step_request = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(*DEFAULT_PROTOCOL_VERSION)
        .with_reward_item(RewardItem::new(
            VALIDATOR_1_PUBLIC_KEY.clone(),
            VALIDATOR_1_REWARD_FACTOR,
        ))
        .with_next_era_id(last_trusted_era.successor())
        .with_era_end_timestamp_millis(timestamp_millis)
        .with_run_auction(true)
        .build();

    builder.step(step_request).expect("should execute step");

    let era_info = {
        let era_info_value = builder
            .query(None, Key::EraInfo(last_trusted_era), &[])
            .expect("should have value");

        era_info_value
            .as_era_info()
            .cloned()
            .expect("should be era info")
    };

    const EXPECTED_VALIDATOR_1_PAYOUT: U512 = U512::zero();

    assert_eq!(
        era_info.seigniorage_allocations().len(),
        PRIVATE_CHAIN_GENESIS_VALIDATORS.len(),
        "running auction should not increase number of validators",
    );

    assert!(
        matches!(
            era_info.select(VALIDATOR_1_PUBLIC_KEY.clone()).next(),
            Some(SeigniorageAllocation::Validator { validator_public_key, amount })
            if *validator_public_key == *VALIDATOR_1_PUBLIC_KEY && *amount == EXPECTED_VALIDATOR_1_PAYOUT
        ),
        "era info is {:?}",
        era_info
    );

    let total_supply_after_distribution = builder.total_supply(None);
    assert_eq!(
        initial_supply, total_supply_after_distribution,
        "total supply of tokens should not increase after an auction is ran"
    )
}
