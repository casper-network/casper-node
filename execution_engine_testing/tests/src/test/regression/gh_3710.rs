use std::{collections::BTreeSet, convert::TryInto, iter::FromIterator};

use casper_engine_test_support::UpgradeRequestBuilder;
use casper_execution_engine::core::engine_state::purge::{PurgeConfig, PurgeResult};
use casper_hashing::Digest;
use casper_types::{EraId, Key, KeyTag, ProtocolVersion};

use crate::lmdb_fixture;

const FIXTURE_N_ERAS: usize = 10;

const GH_3710_FIXTURE: &str = "gh_3710";

#[ignore]
#[test]
fn gh_3710_should_copy_latest_era_info_to_stable_key_at_upgrade_point() {
    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(GH_3710_FIXTURE);

    let auction_delay: u64 = lmdb_fixture_state
        .genesis_request
        .get("ee_config")
        .expect("should have ee_config")
        .get("auction_delay")
        .expect("should have auction delay")
        .as_i64()
        .expect("auction delay should be integer")
        .try_into()
        .expect("auction delay should be positive");

    let last_expected_era_info = EraId::new(auction_delay + FIXTURE_N_ERAS as u64);
    let first_era_after_protocol_upgrade = last_expected_era_info.successor();

    let pre_upgrade_state_root_hash = builder.get_post_state_hash();

    let previous_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let current_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let new_protocol_version = ProtocolVersion::from_parts(
        current_protocol_version.value().major,
        current_protocol_version.value().minor,
        current_protocol_version.value().patch + 1,
    );

    let era_info_keys_before_upgrade = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(
        era_info_keys_before_upgrade.len(),
        auction_delay as usize + 1 + FIXTURE_N_ERAS,
    );

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(previous_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(first_era_after_protocol_upgrade)
            .build()
    };

    builder
        .upgrade_with_upgrade_request(*builder.get_engine_state().config(), &mut upgrade_request)
        .expect_upgrade_success();

    let upgrade_result = builder.get_upgrade_result(0).expect("result");

    let upgrade_success = upgrade_result.as_ref().expect("success");
    assert_eq!(
        upgrade_success.post_state_hash,
        builder.get_post_state_hash(),
        "sanity check"
    );

    let era_info_keys_after_upgrade = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(era_info_keys_after_upgrade, era_info_keys_before_upgrade);

    let last_era_info_value = builder
        .query(
            Some(pre_upgrade_state_root_hash),
            Key::EraInfo(last_expected_era_info),
            &[],
        )
        .expect("should query pre-upgrade stored value");

    let era_summary = builder
        .query(None, Key::EraSummary, &[])
        .expect("should query stable key after the upgrade");

    assert_eq!(last_era_info_value, era_summary);
}

#[ignore]
#[test]
fn gh_3710_commit_purge_with_empty_keys_should_be_noop() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(GH_3710_FIXTURE);

    let purge_config = PurgeConfig::new(builder.get_post_state_hash(), Vec::new());

    builder.commit_purge(purge_config).expect_purge_success();
}

#[ignore]
#[test]
fn gh_3710_commit_purge_should_validate_state_root_hash() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(GH_3710_FIXTURE);

    let purge_config = PurgeConfig::new(Digest::hash("foobar"), Vec::new());

    builder.commit_purge(purge_config);

    let purge_result = builder
        .get_purge_result(0)
        .expect("should have purge result");
    assert!(builder.get_purge_result(1).is_none());

    assert!(
        matches!(purge_result, Ok(PurgeResult::RootNotFound)),
        "{:?}",
        purge_result
    );
}

#[ignore]
#[test]
fn gh_3710_commit_purge_should_delete_values() {
    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(GH_3710_FIXTURE);

    let auction_delay: u64 = lmdb_fixture_state
        .genesis_request
        .get("ee_config")
        .expect("should have ee_config")
        .get("auction_delay")
        .expect("should have auction delay")
        .as_i64()
        .expect("auction delay should be integer")
        .try_into()
        .expect("auction delay should be positive");

    let keys_before_purge = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should obtain all given keys");

    assert_eq!(
        keys_before_purge.len(),
        FIXTURE_N_ERAS + 1 + auction_delay as usize
    );

    let batch_1: Vec<Key> = (0..FIXTURE_N_ERAS)
        .map(|i| EraId::new(i.try_into().unwrap()))
        .map(Key::EraInfo)
        .collect();

    let batch_2: Vec<Key> = (FIXTURE_N_ERAS..FIXTURE_N_ERAS + 1 + auction_delay as usize)
        .map(|i| EraId::new(i.try_into().unwrap()))
        .map(Key::EraInfo)
        .collect();

    assert_eq!(
        BTreeSet::from_iter(batch_1.iter())
            .union(&BTreeSet::from_iter(batch_2.iter()))
            .collect::<BTreeSet<_>>()
            .len(),
        keys_before_purge.len(),
        "sanity check"
    );

    // Process purge of first batch
    let pre_state_hash = builder.get_post_state_hash();

    let purge_config_1 = PurgeConfig::new(pre_state_hash, batch_1);

    builder.commit_purge(purge_config_1).expect_purge_success();
    let post_state_hash_batch_1 = builder.get_post_state_hash();
    assert_ne!(pre_state_hash, post_state_hash_batch_1);

    let keys_after_batch_1_purge = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should obtain all given keys");

    assert_eq!(keys_after_batch_1_purge.len(), 2);

    // Process purge of second batch
    let pre_state_hash = builder.get_post_state_hash();

    let purge_config_2 = PurgeConfig::new(pre_state_hash, batch_2);
    builder.commit_purge(purge_config_2).expect_purge_success();
    let post_state_hash_batch_2 = builder.get_post_state_hash();
    assert_ne!(pre_state_hash, post_state_hash_batch_2);

    let keys_after_batch_2_purge = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should obtain all given keys");

    assert_eq!(keys_after_batch_2_purge.len(), 0);
}

#[cfg(feature = "fixture-generators")]
mod fixture {
    use std::collections::BTreeMap;

    use casper_engine_test_support::{
        ExecuteRequestBuilder, StepRequestBuilder, DEFAULT_ACCOUNT_ADDR,
        DEFAULT_ACCOUNT_PUBLIC_KEY, PRODUCTION_RUN_GENESIS_REQUEST,
    };
    use casper_execution_engine::core::engine_state::RewardItem;
    use casper_types::{
        runtime_args,
        system::auction::{self, DelegationRate, EraInfo, SeigniorageAllocation},
        EraId, Key, KeyTag, ProtocolVersion, RuntimeArgs, StoredValue, U512,
    };

    use super::{FIXTURE_N_ERAS, GH_3710_FIXTURE};
    use crate::lmdb_fixture;

    #[test]
    fn generate_era_info_bloat_fixture() {
        // To generate this fixture again you have to re-run this code release-1.4.13.
        let genesis_request = PRODUCTION_RUN_GENESIS_REQUEST.clone();
        lmdb_fixture::generate_fixture(GH_3710_FIXTURE, genesis_request, |builder| {
            const DELEGATION_RATE: DelegationRate = 10;

            let args = runtime_args! {
                auction::ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
                auction::ARG_DELEGATION_RATE => DELEGATION_RATE,
                auction::ARG_AMOUNT => U512::from(1_000_000u64),
            };

            let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                builder.get_auction_contract_hash(),
                auction::METHOD_ADD_BID,
                args,
            )
            .build();

            builder.exec(add_bid_request).expect_success().commit();

            // compute N eras

            let current_era_id = builder.get_era();

            // eras current..=delay + 1 without rewards (default genesis validator is not a
            // validator yet)
            for era_counter in current_era_id.iter(builder.get_auction_delay() + 1) {
                let step_request = StepRequestBuilder::new()
                    .with_parent_state_hash(builder.get_post_state_hash())
                    .with_protocol_version(ProtocolVersion::V1_0_0)
                    .with_next_era_id(era_counter)
                    // no rewards as default validator is not a validator yet
                    .build();
                builder.step(step_request).unwrap();
            }

            let current_era_id = builder.get_era();
            for era_counter in current_era_id.iter(FIXTURE_N_ERAS as u64) {
                let value = era_counter.value() * 1_000_000u64;
                let step_request = StepRequestBuilder::new()
                    .with_parent_state_hash(builder.get_post_state_hash())
                    .with_protocol_version(ProtocolVersion::V1_0_0)
                    .with_next_era_id(era_counter)
                    .with_reward_item(RewardItem::new(DEFAULT_ACCOUNT_PUBLIC_KEY.clone(), value))
                    .build();
                builder.step(step_request).unwrap();
            }

            // N more eras that pays out rewards

            let last_era_info = EraId::new(builder.get_auction_delay() + FIXTURE_N_ERAS as u64);
            let last_era_info_key = Key::EraInfo(last_era_info);

            let keys = builder.get_keys(KeyTag::EraInfo).unwrap();
            let mut keys_lookup = BTreeMap::new();
            for key in &keys {
                keys_lookup.insert(key, ());
            }

            assert!(keys_lookup.contains_key(&last_era_info_key));
            assert_eq!(keys_lookup.keys().last().copied(), Some(&last_era_info_key));

            // all era infos should have unique rewards that are in increasing order
            let stored_values: Vec<StoredValue> = keys_lookup
                .keys()
                .map(|key| builder.query(None, **key, &[]).unwrap())
                .collect();

            let era_infos: Vec<&EraInfo> = stored_values
                .iter()
                .filter_map(StoredValue::as_era_info)
                .collect();

            let rewards: Vec<&U512> = era_infos
                .iter()
                .flat_map(|era_info| era_info.seigniorage_allocations())
                .filter_map(|seigniorage| match seigniorage {
                    SeigniorageAllocation::Validator {
                        validator_public_key,
                        amount,
                    } if validator_public_key == &*DEFAULT_ACCOUNT_PUBLIC_KEY => Some(amount),
                    SeigniorageAllocation::Validator { .. } => panic!("Unexpected validator"),
                    SeigniorageAllocation::Delegator { .. } => panic!("No delegators"),
                })
                .collect();

            let sorted_rewards = {
                let mut vec = rewards.clone();
                vec.sort();
                vec
            };
            assert_eq!(rewards, sorted_rewards);

            assert!(
                rewards.first().unwrap() < rewards.last().unwrap(),
                "{:?}",
                rewards
            );
        })
        .unwrap();
    }
}
