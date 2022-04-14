use std::collections::BTreeMap;

use casper_engine_test_support::{
    ExecuteRequestBuilder, DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
    SYSTEM_ADDR, TIMESTAMP_MILLIS_INCREMENT,
};
use casper_types::{
    runtime_args,
    system::auction::{self, SeigniorageAllocation},
    Key, PublicKey, RuntimeArgs, U512,
};

use crate::test::private_chain::{PRIVATE_CHAIN_GENESIS_VALIDATORS, VALIDATOR_1_PUBLIC_KEY};

const CONTRACT_AUCTION_BIDS: &str = "auction_bids.wasm";
const ARG_ENTRY_POINT: &str = "entry_point";

#[ignore]
#[test]
fn should_not_distribute_rewards_but_compute_next_set() {
    const VALIDATOR_1_REWARD_FACTOR: u64 = 0;

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = super::private_chain_setup();

    // initial token supply
    let initial_supply = builder.total_supply(None);

    for _ in 0..5 {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    let reward_factors: BTreeMap<PublicKey, u64> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(VALIDATOR_1_PUBLIC_KEY.clone(), VALIDATOR_1_REWARD_FACTOR);
        // tmp.insert(VALIDATOR_2.clone(), VALIDATOR_2_REWARD_FACTOR);
        // tmp.insert(VALIDATOR_3.clone(), VALIDATOR_3_REWARD_FACTOR);
        tmp
    };

    let distribute_request = ExecuteRequestBuilder::standard(
        *SYSTEM_ADDR,
        CONTRACT_AUCTION_BIDS,
        runtime_args! {
            ARG_ENTRY_POINT => auction::METHOD_DISTRIBUTE,
            auction::ARG_REWARD_FACTORS => reward_factors
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let era_info = {
        let era = builder.get_era();

        let era_info_value = builder
            .query(None, Key::EraInfo(era), &[])
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
