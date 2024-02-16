use std::{collections::BTreeSet, convert::TryInto, iter::FromIterator};

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, StepRequestBuilder, WasmTestBuilder,
    DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_PROPOSER_PUBLIC_KEY, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::{PruneConfig, PruneResult};
use casper_storage::global_state::state::{CommitProvider, StateProvider};
use casper_types::{
    runtime_args,
    system::auction::{self, DelegationRate},
    Digest, EraId, Key, KeyTag, ProtocolVersion, PublicKey, U512,
};

use crate::lmdb_fixture;

const FIXTURE_N_ERAS: usize = 10;

const GH_3710_FIXTURE: &str = "gh_3710";

#[ignore]
#[test]
fn gh_3710_commit_prune_with_empty_keys_should_be_noop() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(GH_3710_FIXTURE);

    let prune_config = PruneConfig::new(builder.get_post_state_hash(), Vec::new());

    builder.commit_prune(prune_config).expect_prune_success();
}

#[ignore]
#[test]
fn gh_3710_commit_prune_should_validate_state_root_hash() {
    let (mut builder, _lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(GH_3710_FIXTURE);

    let prune_config = PruneConfig::new(Digest::hash("foobar"), Vec::new());

    builder.commit_prune(prune_config);

    let prune_result = builder
        .get_prune_result(0)
        .expect("should have prune result");
    assert!(builder.get_prune_result(1).is_none());

    assert!(
        matches!(prune_result, Ok(PruneResult::RootNotFound)),
        "{:?}",
        prune_result
    );
}

#[ignore]
#[test]
fn gh_3710_commit_prune_should_delete_values() {
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

    let keys_before_prune = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should obtain all given keys");

    assert_eq!(
        keys_before_prune.len(),
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
        keys_before_prune.len(),
        "sanity check"
    );

    // Process prune of first batch
    let pre_state_hash = builder.get_post_state_hash();

    let prune_config_1 = PruneConfig::new(pre_state_hash, batch_1);

    builder.commit_prune(prune_config_1).expect_prune_success();
    let post_state_hash_batch_1 = builder.get_post_state_hash();
    assert_ne!(pre_state_hash, post_state_hash_batch_1);

    let keys_after_batch_1_prune = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should obtain all given keys");

    assert_eq!(keys_after_batch_1_prune.len(), 2);

    // Process prune of second batch
    let pre_state_hash = builder.get_post_state_hash();

    let prune_config_2 = PruneConfig::new(pre_state_hash, batch_2);
    builder.commit_prune(prune_config_2).expect_prune_success();
    let post_state_hash_batch_2 = builder.get_post_state_hash();
    assert_ne!(pre_state_hash, post_state_hash_batch_2);

    let keys_after_batch_2_prune = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should obtain all given keys");

    assert_eq!(keys_after_batch_2_prune.len(), 0);
}

const DEFAULT_REWARD_AMOUNT: u64 = 1_000_000;

fn add_validator_and_wait_for_rotation<S>(builder: &mut WasmTestBuilder<S>, public_key: &PublicKey)
where
    S: StateProvider + CommitProvider,
{
    const DELEGATION_RATE: DelegationRate = 10;

    let args = runtime_args! {
        auction::ARG_PUBLIC_KEY => public_key.clone(),
        auction::ARG_DELEGATION_RATE => DELEGATION_RATE,
        auction::ARG_AMOUNT => U512::from(DEFAULT_REWARD_AMOUNT),
    };

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        public_key.to_account_hash(),
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
}

fn distribute_rewards<S>(
    builder: &mut WasmTestBuilder<S>,
    block_height: u64,
    proposer: &PublicKey,
    amount: U512,
) where
    S: StateProvider + CommitProvider,
{
    builder
        .distribute(
            None,
            ProtocolVersion::V1_0_0,
            &IntoIterator::into_iter([(proposer.clone(), amount)]).collect(),
            block_height,
            0,
        )
        .unwrap();
}

#[ignore]
#[test]
fn gh_3710_should_produce_era_summary_in_a_step() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    add_validator_and_wait_for_rotation(&mut builder, &DEFAULT_ACCOUNT_PUBLIC_KEY);
    distribute_rewards(&mut builder, 1, &DEFAULT_ACCOUNT_PUBLIC_KEY, 0.into());

    let era_info_keys = builder.get_keys(KeyTag::EraInfo).unwrap();
    assert_eq!(era_info_keys, Vec::new());

    let era_summary_1 = builder
        .query(None, Key::EraSummary, &[])
        .expect("should query era summary");

    let era_summary_1 = era_summary_1.as_era_info().expect("era summary");

    // Reward another validator to observe that the summary changes.
    add_validator_and_wait_for_rotation(&mut builder, &DEFAULT_PROPOSER_PUBLIC_KEY);
    distribute_rewards(&mut builder, 2, &DEFAULT_PROPOSER_PUBLIC_KEY, 1.into());

    let era_summary_2 = builder
        .query(None, Key::EraSummary, &[])
        .expect("should query era summary");

    let era_summary_2 = era_summary_2.as_era_info().expect("era summary");

    assert_ne!(era_summary_1, era_summary_2);

    let era_info_keys = builder.get_keys(KeyTag::EraInfo).unwrap();
    assert_eq!(era_info_keys, Vec::new());

    // As a sanity check ensure there's just a single era summary per tip
    assert_eq!(
        builder
            .get_keys(KeyTag::EraSummary)
            .expect("should get all era summary keys")
            .len(),
        1
    );
}

mod fixture {
    use std::collections::BTreeMap;

    use casper_engine_test_support::{
        ExecuteRequestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_PUBLIC_KEY,
        PRODUCTION_RUN_GENESIS_REQUEST,
    };
    use casper_types::{
        runtime_args,
        system::auction::{EraInfo, SeigniorageAllocation},
        EraId, Key, KeyTag, StoredValue, U512,
    };

    use super::{FIXTURE_N_ERAS, GH_3710_FIXTURE};
    use crate::lmdb_fixture;

    #[ignore = "RUN_FIXTURE_GENERATORS env var should be enabled"]
    #[test]
    fn generate_call_stack_fixture() {
        const CALL_STACK_FIXTURE: &str = "call_stack_fixture";
        const CONTRACT_RECURSIVE_SUBCALL: &str = "get_call_stack_recursive_subcall.wasm";

        if !lmdb_fixture::is_fixture_generator_enabled() {
            println!("Enable the RUN_FIXTURE_GENERATORS variable");
            return;
        }

        let genesis_request = PRODUCTION_RUN_GENESIS_REQUEST.clone();

        lmdb_fixture::generate_fixture(CALL_STACK_FIXTURE, genesis_request, |builder| {
            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_RECURSIVE_SUBCALL,
                runtime_args! {},
            )
            .build();

            builder.exec(execute_request).expect_success().commit();
        })
        .unwrap();
    }

    #[ignore = "RUN_FIXTURE_GENERATORS env var should be enabled"]
    #[test]
    fn generate_groups_fixture() {
        const GROUPS_FIXTURE: &str = "groups";
        const GROUPS_WASM: &str = "groups.wasm";

        if !lmdb_fixture::is_fixture_generator_enabled() {
            println!("Enable the RUN_FIXTURE_GENERATORS variable");
            return;
        }

        let genesis_request = PRODUCTION_RUN_GENESIS_REQUEST.clone();

        lmdb_fixture::generate_fixture(GROUPS_FIXTURE, genesis_request, |builder| {
            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                GROUPS_WASM,
                runtime_args! {},
            )
            .build();

            builder.exec(execute_request).expect_success().commit();
        })
        .unwrap();
    }

    #[ignore = "RUN_FIXTURE_GENERATORS env var should be enabled"]
    #[test]
    fn generate_era_info_bloat_fixture() {
        if !lmdb_fixture::is_fixture_generator_enabled() {
            println!("Enable the RUN_FIXTURE_GENERATORS variable");
            return;
        }
        // To generate this fixture again you have to re-run this code release-1.4.13.
        let genesis_request = PRODUCTION_RUN_GENESIS_REQUEST.clone();
        lmdb_fixture::generate_fixture(GH_3710_FIXTURE, genesis_request, |builder| {
            super::add_validator_and_wait_for_rotation(builder, &DEFAULT_ACCOUNT_PUBLIC_KEY);

            // N more eras that pays out rewards
            super::distribute_rewards(builder, 0, &DEFAULT_ACCOUNT_PUBLIC_KEY, 0.into());

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
