use std::{convert::TryInto, fmt};

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, StepRequestBuilder, WasmTestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_PUBLIC_KEY, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{
    core::{
        engine_state::{
            self,
            migrate::{MigrationAction, MigrationActions},
            RewardItem,
        },
        execution,
    },
    storage::global_state::{CommitProvider, StateProvider},
};
use casper_types::{
    runtime_args,
    system::auction::{self, DelegationRate},
    EraId, Key, KeyTag, ProtocolVersion, RuntimeArgs, U512,
};

use crate::lmdb_fixture;

const _DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);
const FIXTURE_N_ERAS: usize = 10;

const GH_3710_FIXTURE: &str = "gh_3710";
const DEFAULT_REWARD_VALUE: u64 = 1_000_000;

#[ignore]
#[test]
fn gh_3710_should_delete_eras_on_each_migration_step() {
    const BATCH_SIZE: u32 = 4;

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

    let last_era_id = EraId::new(auction_delay + FIXTURE_N_ERAS as u64);

    // We'll supply last known era info id that so it is guaranteed that next era is not found in
    // the global state.
    const CURRENT_ERA_ID: u64 = FIXTURE_N_ERAS as u64 + 1;

    let era_info_before_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(
        era_info_before_migration.last(),
        Some(&Key::EraInfo(EraId::new(CURRENT_ERA_ID as u64))),
    );

    dbg!(&era_info_before_migration);
    assert_eq!(
        era_info_before_migration.last(),
        Some(&Key::EraInfo(last_era_id))
    );

    let current_root_hash = builder.get_post_state_hash();

    // Migrate step 1 - delete 5 keys (0..5)

    let action = MigrationAction::purge_era_info(BATCH_SIZE, CURRENT_ERA_ID);

    builder
        .commit_migrate(MigrationActions::new(0, current_root_hash, vec![action]))
        .expect_migrate_success();

    assert_ne!(current_root_hash, builder.get_post_state_hash());

    let era_info_after_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(
        era_info_after_migration.first(),
        Some(&Key::EraInfo(EraId::new(BATCH_SIZE as u64))),
    );

    // Migrate step 2 - delete 5 keys (5..10)

    let current_root_hash = builder.get_post_state_hash();
    let action = MigrationAction::purge_era_info(BATCH_SIZE, CURRENT_ERA_ID);

    builder
        .commit_migrate(MigrationActions::new(0, current_root_hash, vec![action]))
        .expect_migrate_success();

    assert_ne!(current_root_hash, builder.get_post_state_hash());

    let era_info_after_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(
        era_info_after_migration.first(),
        Some(&Key::EraInfo(EraId::new(
            BATCH_SIZE as u64 + BATCH_SIZE as u64
        ))),
    );

    // Migrate step 3 - delete 2 keys (10..12)

    let current_root_hash = builder.get_post_state_hash();
    let action = MigrationAction::purge_era_info(BATCH_SIZE, CURRENT_ERA_ID);
    builder
        .commit_migrate(MigrationActions::new(0, current_root_hash, vec![action]))
        .expect_migrate_success();

    assert_ne!(current_root_hash, builder.get_post_state_hash());

    let era_info_after_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(era_info_after_migration.first(), None,);

    // Migrate step 4 - should be noop, and should not fail

    let current_root_hash = builder.get_post_state_hash();
    let action = MigrationAction::purge_era_info(BATCH_SIZE, CURRENT_ERA_ID);
    builder
        .commit_migrate(MigrationActions::new(0, current_root_hash, vec![action]))
        .expect_migrate_success();

    let last_migration_run = builder
        .get_keys(KeyTag::LastMigration)
        .expect("should return all the era info keys");

    assert_eq!(last_migration_run.first(), Some(&Key::LastMigration));
    let era_info_after_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");
    assert_eq!(era_info_after_migration.first(), None,);

    assert_ne!(
        current_root_hash,
        builder.get_post_state_hash(),
        "Post state hash should change if last_migration_run is written"
    );

    let era_info_after_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(era_info_after_migration.first(), None,);
}

#[ignore]
#[test]
fn gh_3710_should_delete_eras_on_each_migration_step_with_increasing_eras() {
    const BATCH_SIZE: u32 = 4;

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

    let last_era_id = EraId::new(auction_delay + FIXTURE_N_ERAS as u64);

    // We'll supply last known era info id that so it is guaranteed that next era is not found in
    // the global state.
    const CURRENT_ERA_ID: u64 = FIXTURE_N_ERAS as u64 + 1;

    let era_info_before_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(
        era_info_before_migration.last(),
        Some(&Key::EraInfo(EraId::new(CURRENT_ERA_ID as u64))),
    );

    assert_eq!(
        era_info_before_migration.last(),
        Some(&Key::EraInfo(last_era_id))
    );

    let current_root_hash = builder.get_post_state_hash();

    // Migrate step 1 - delete 5 keys (0..5)

    let action = MigrationAction::purge_era_info(BATCH_SIZE, CURRENT_ERA_ID);

    builder
        .commit_migrate(MigrationActions::new(0, current_root_hash, vec![action]))
        .expect_migrate_success();

    assert_ne!(current_root_hash, builder.get_post_state_hash());

    let era_info_after_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(
        era_info_after_migration.first(),
        Some(&Key::EraInfo(EraId::new(BATCH_SIZE as u64))),
    );

    // Migrate step 2 - delete 5 keys (5..10)

    let current_root_hash = builder.get_post_state_hash();
    let action = MigrationAction::purge_era_info(BATCH_SIZE, CURRENT_ERA_ID + 100);

    builder
        .commit_migrate(MigrationActions::new(0, current_root_hash, vec![action]))
        .expect_migrate_success();

    assert_ne!(current_root_hash, builder.get_post_state_hash());

    let era_info_after_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(
        era_info_after_migration.first(),
        Some(&Key::EraInfo(EraId::new(
            BATCH_SIZE as u64 + BATCH_SIZE as u64
        ))),
    );

    // Migrate step 3 - delete 2 keys (10..12)

    let current_root_hash = builder.get_post_state_hash();
    let action = MigrationAction::purge_era_info(BATCH_SIZE, CURRENT_ERA_ID + 200);
    builder
        .commit_migrate(MigrationActions::new(0, current_root_hash, vec![action]))
        .expect_migrate_success();

    assert_ne!(current_root_hash, builder.get_post_state_hash());

    let era_info_after_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(era_info_after_migration.first(), None,);

    // Migrate step 4 - should be noop, and should not fail

    let current_root_hash = builder.get_post_state_hash();
    let action = MigrationAction::purge_era_info(BATCH_SIZE, CURRENT_ERA_ID + 300);
    builder
        .commit_migrate(MigrationActions::new(0, current_root_hash, vec![action]))
        .expect_migrate_success();

    let last_migration_run = builder
        .get_keys(KeyTag::LastMigration)
        .expect("should return all the era info keys");

    assert_eq!(last_migration_run.first(), Some(&Key::LastMigration));
    let era_info_after_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");
    assert_eq!(era_info_after_migration.first(), None,);

    assert_ne!(
        current_root_hash,
        builder.get_post_state_hash(),
        "Post state hash should change if last_migration_run is written"
    );

    let era_info_after_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(era_info_after_migration.first(), None,);
}

#[ignore]
#[test]
fn gh_3710_should_not_perform_migration_if_it_has_already_run() {
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

    let last_era_id = EraId::new(auction_delay + FIXTURE_N_ERAS as u64);

    let era_info_before_migration = builder
        .get_keys(KeyTag::EraInfo)
        .expect("should return all the era info keys");

    assert_eq!(
        era_info_before_migration.last(),
        Some(&Key::EraInfo(last_era_id))
    );

    let current_root_hash = builder.get_post_state_hash();
    let action = MigrationAction::write_stable_era_info(last_era_id);
    builder
        .commit_migrate(MigrationActions::new(1, current_root_hash, vec![action]))
        .expect_migrate_success();

    assert_ne!(current_root_hash, builder.get_post_state_hash());

    let current_root_hash = builder.get_post_state_hash();
    let action = MigrationAction::write_stable_era_info(last_era_id);
    builder
        .commit_migrate(MigrationActions::new(1, current_root_hash, vec![action]))
        .expect_migrate_success();

    let era_summary_after_migration = builder
        .get_keys(KeyTag::EraSummary)
        .expect("should return the current era summary");

    assert_ne!(current_root_hash, builder.get_post_state_hash());

    assert!(matches!(
        era_summary_after_migration.first(),
        Some(Key::EraSummary { .. })
    ))
}

#[ignore]
#[test]
fn gh_3710_should_produce_era_summary_in_a_step() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    add_validator_and_wait_for_rotation(&mut builder);
    progress_eras(&mut builder, DEFAULT_REWARD_VALUE, FIXTURE_N_ERAS);

    let era_info_keys = builder.get_keys(KeyTag::EraInfo).unwrap();
    assert_eq!(era_info_keys, Vec::new());

    let era_summary_1 = builder
        .query(None, Key::EraSummary, &[])
        .expect("should query era summary");

    let era_summary_1 = era_summary_1.as_era_info().expect("era summary");

    // Double the reward in next era to observe that the summary changes.
    progress_eras(&mut builder, DEFAULT_REWARD_VALUE * 2, 1);

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

fn add_validator_and_wait_for_rotation<S>(builder: &mut WasmTestBuilder<S>)
where
    S: StateProvider + CommitProvider,
    engine_state::Error: From<S::Error>,
    S::Error: Into<execution::Error> + fmt::Debug,
{
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

    // eras current..=delay + 1 without rewards
    // default genesis validator is not a validator yet
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

fn progress_eras<S>(builder: &mut WasmTestBuilder<S>, reward_value: u64, era_count: usize)
where
    S: StateProvider + CommitProvider,
    engine_state::Error: From<S::Error>,
    S::Error: Into<execution::Error>,
{
    let current_era_id = builder.get_era();
    for era_counter in current_era_id.iter(era_count as u64) {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(era_counter)
            .with_reward_item(RewardItem::new(
                DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
                reward_value,
            ))
            .build();
        builder.step(step_request).unwrap();
    }
}

//#[cfg(feature = "fixture-generators")]
mod fixture {
    use std::collections::BTreeMap;

    use casper_engine_test_support::PRODUCTION_RUN_GENESIS_REQUEST;
    use casper_types::{EraId, Key, KeyTag};

    use super::GH_3710_FIXTURE;
    use crate::{
        lmdb_fixture,
        test::regression::gh_3710::{DEFAULT_REWARD_VALUE, FIXTURE_N_ERAS},
    };

    #[test]
    fn generate_era_info_bloat_fixture() {
        // To generate this fixture again you have to re-run this code release-1.4.13.
        let genesis_request = PRODUCTION_RUN_GENESIS_REQUEST.clone();
        lmdb_fixture::generate_fixture(GH_3710_FIXTURE, genesis_request, |builder| {
            super::add_validator_and_wait_for_rotation(builder);

            // N more eras that pays out rewards
            super::progress_eras(builder, DEFAULT_REWARD_VALUE, FIXTURE_N_ERAS);

            let last_era_info = EraId::new(builder.get_auction_delay() + FIXTURE_N_ERAS as u64);
            let last_era_info_key = Key::EraInfo(last_era_info);

            let keys = builder.get_keys(KeyTag::EraInfo).unwrap();
            let mut keys_lookup = BTreeMap::new();
            for key in &keys {
                keys_lookup.insert(key, ());
            }

            assert!(keys_lookup.contains_key(&last_era_info_key));
            assert_eq!(keys_lookup.keys().last().copied(), Some(&last_era_info_key));
        })
        .unwrap();
    }
}
