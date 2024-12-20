use std::collections::BTreeMap;

use num_rational::Ratio;

use casper_engine_test_support::{
    ChainspecConfig, ExecuteRequestBuilder, LmdbWasmTestBuilder, UpgradeRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_MAX_ASSOCIATED_KEYS, DEFAULT_UNBONDING_DELAY,
    LOCAL_GENESIS_REQUEST,
};

use crate::{lmdb_fixture, lmdb_fixture::ENTRY_REGISTRY_SPECIAL_ADDRESS};
use casper_types::{
    account::{AccountHash, ACCOUNT_HASH_LENGTH},
    contracts::NamedKeys,
    runtime_args,
    system::{
        self,
        auction::{
            DelegatorKind, SeigniorageRecipientsSnapshotV1, SeigniorageRecipientsSnapshotV2,
            AUCTION_DELAY_KEY, DEFAULT_SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION,
            LOCKED_FUNDS_PERIOD_KEY, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION_KEY, UNBONDING_DELAY_KEY, VALIDATOR_SLOTS_KEY,
        },
        mint::ROUND_SEIGNIORAGE_RATE_KEY,
    },
    Account, AddressableEntityHash, CLValue, CoreConfig, EntityAddr, EraId, Key, ProtocolVersion,
    StorageCosts, StoredValue, SystemHashRegistry, U256, U512,
};
use rand::Rng;

const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V1_0_0;
const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);
const ARG_ACCOUNT: &str = "account";

#[ignore]
#[test]
fn should_upgrade_only_protocol_version() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    // let old_wasm_config = *builder.get_engine_state().config().wasm_config();

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    // let upgraded_engine_config = builder.get_engine_state().config();
    //
    // assert_eq!(
    //     old_wasm_config,
    //     *upgraded_engine_config.wasm_config(),
    //     "upgraded costs should equal original costs"
    // );
}

#[ignore]
#[test]
fn should_allow_only_wasm_costs_patch_version() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 2);

    // let new_wasm_config = get_upgraded_wasm_config();

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    // let upgraded_engine_config = builder.get_engine_state().config();

    // assert_eq!(
    //     new_wasm_config,
    //     *upgraded_engine_config.wasm_config(),
    //     "upgraded costs should equal new costs"
    // );
}

#[ignore]
#[test]
fn should_allow_only_wasm_costs_minor_version() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor + 1, sem_ver.patch);

    // let new_wasm_config = get_upgraded_wasm_config();

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    // let engine_config = EngineConfigBuilder::default()
    //     .with_wasm_config(new_wasm_config)
    //     .build();

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();
    //
    // let upgraded_engine_config = builder.get_engine_state().config();
    //
    // assert_eq!(
    //     new_wasm_config,
    //     *upgraded_engine_config.wasm_config(),
    //     "upgraded costs should equal new costs"
    // );
}

#[ignore]
#[test]
fn should_not_downgrade() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    // let old_wasm_config = *builder.get_engine_state().config().wasm_config();

    let new_protocol_version = ProtocolVersion::from_parts(2, 0, 0);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    let mut downgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(new_protocol_version)
            .with_new_protocol_version(PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade(&mut downgrade_request);

    let upgrade_result = builder.get_upgrade_result(1).expect("should have response");

    assert!(
        !upgrade_result.is_success(),
        "expected failure got {:?}",
        upgrade_result
    );
}

#[ignore]
#[test]
fn should_not_skip_major_versions() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let sem_ver = PROTOCOL_VERSION.value();

    let invalid_version =
        ProtocolVersion::from_parts(sem_ver.major + 2, sem_ver.minor, sem_ver.patch);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(invalid_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade(&mut upgrade_request);

    let upgrade_result = builder.get_upgrade_result(0).expect("should have response");

    assert!(upgrade_result.is_err(), "expected failure");
}

#[ignore]
#[test]
fn should_allow_skip_minor_versions() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let sem_ver = PROTOCOL_VERSION.value();

    // can skip minor versions as long as they are higher than current version
    let valid_new_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor + 2, sem_ver.patch);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(valid_new_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    builder.upgrade(&mut upgrade_request);

    let upgrade_result = builder.get_upgrade_result(0).expect("should have response");

    assert!(upgrade_result.is_success(), "expected success");
}

#[ignore]
#[test]
fn should_upgrade_only_validator_slots() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    let validator_slot_key = *builder
        .get_named_keys(EntityAddr::System(
            builder.get_auction_contract_hash().value(),
        ))
        .get(VALIDATOR_SLOTS_KEY)
        .unwrap();

    let before_validator_slots: u32 = builder
        .query(None, validator_slot_key, &[])
        .expect("should have validator slots")
        .as_cl_value()
        .expect("should be CLValue")
        .clone()
        .into_t()
        .expect("should be u32");

    let new_validator_slots = before_validator_slots + 1;

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_new_validator_slots(new_validator_slots)
            .build()
    };

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    let after_validator_slots: u32 = builder
        .query(None, validator_slot_key, &[])
        .expect("should have validator slots")
        .as_cl_value()
        .expect("should be CLValue")
        .clone()
        .into_t()
        .expect("should be u32");

    assert_eq!(
        new_validator_slots, after_validator_slots,
        "should have upgraded validator slots to expected value"
    )
}

#[ignore]
#[test]
fn should_upgrade_only_auction_delay() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    let auction_delay_key = *builder
        .get_named_keys(EntityAddr::System(
            builder.get_auction_contract_hash().value(),
        ))
        .get(AUCTION_DELAY_KEY)
        .unwrap();

    let before_auction_delay: u64 = builder
        .query(None, auction_delay_key, &[])
        .expect("should have auction delay")
        .as_cl_value()
        .expect("should be a CLValue")
        .clone()
        .into_t()
        .expect("should be u64");

    let new_auction_delay = before_auction_delay + 1;

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_new_auction_delay(new_auction_delay)
            .build()
    };

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    let after_auction_delay: u64 = builder
        .query(None, auction_delay_key, &[])
        .expect("should have auction delay")
        .as_cl_value()
        .expect("should be a CLValue")
        .clone()
        .into_t()
        .expect("should be u64");

    assert_eq!(
        new_auction_delay, after_auction_delay,
        "should hae upgrade version auction delay"
    )
}

#[ignore]
#[test]
fn should_upgrade_only_locked_funds_period() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    let locked_funds_period_key = *builder
        .get_named_keys(EntityAddr::System(
            builder.get_auction_contract_hash().value(),
        ))
        .get(LOCKED_FUNDS_PERIOD_KEY)
        .unwrap();

    let before_locked_funds_period_millis: u64 = builder
        .query(None, locked_funds_period_key, &[])
        .expect("should have locked funds period")
        .as_cl_value()
        .expect("should be a CLValue")
        .clone()
        .into_t()
        .expect("should be u64");

    let new_locked_funds_period_millis = before_locked_funds_period_millis + 1;

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_new_locked_funds_period_millis(new_locked_funds_period_millis)
            .build()
    };

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    let after_locked_funds_period_millis: u64 = builder
        .query(None, locked_funds_period_key, &[])
        .expect("should have locked funds period")
        .as_cl_value()
        .expect("should be a CLValue")
        .clone()
        .into_t()
        .expect("should be u64");

    assert_eq!(
        new_locked_funds_period_millis, after_locked_funds_period_millis,
        "Should have upgraded locked funds period"
    )
}

#[ignore]
#[test]
fn should_upgrade_only_round_seigniorage_rate() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    let keys = builder.get_named_keys(EntityAddr::System(builder.get_mint_contract_hash().value()));

    let round_seigniorage_rate_key = *keys.get(ROUND_SEIGNIORAGE_RATE_KEY).unwrap();

    let before_round_seigniorage_rate: Ratio<U512> = builder
        .query(None, round_seigniorage_rate_key, &[])
        .expect("should have locked funds period")
        .as_cl_value()
        .expect("should be a CLValue")
        .clone()
        .into_t()
        .expect("should be u64");

    let new_round_seigniorage_rate = Ratio::new(1, 1_000_000_000);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_new_round_seigniorage_rate(new_round_seigniorage_rate)
            .build()
    };

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    let after_round_seigniorage_rate: Ratio<U512> = builder
        .query(None, round_seigniorage_rate_key, &[])
        .expect("should have locked funds period")
        .as_cl_value()
        .expect("should be a CLValue")
        .clone()
        .into_t()
        .expect("should be u64");

    assert_ne!(before_round_seigniorage_rate, after_round_seigniorage_rate);

    let expected_round_seigniorage_rate = Ratio::new(
        U512::from(*new_round_seigniorage_rate.numer()),
        U512::from(*new_round_seigniorage_rate.denom()),
    );

    assert_eq!(
        expected_round_seigniorage_rate, after_round_seigniorage_rate,
        "Should have upgraded locked funds period"
    );
}

#[ignore]
#[test]
fn should_upgrade_only_unbonding_delay() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    let entity_addr = EntityAddr::System(builder.get_auction_contract_hash().value());

    let unbonding_delay_key = *builder
        .get_named_keys(entity_addr)
        .get(UNBONDING_DELAY_KEY)
        .unwrap();

    let before_unbonding_delay: u64 = builder
        .query(None, unbonding_delay_key, &[])
        .expect("should have locked funds period")
        .as_cl_value()
        .expect("should be a CLValue")
        .clone()
        .into_t()
        .expect("should be u64");

    let new_unbonding_delay = DEFAULT_UNBONDING_DELAY + 5;

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_new_unbonding_delay(new_unbonding_delay)
            .build()
    };

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    let after_unbonding_delay: u64 = builder
        .query(None, unbonding_delay_key, &[])
        .expect("should have locked funds period")
        .as_cl_value()
        .expect("should be a CLValue")
        .clone()
        .into_t()
        .expect("should be u64");

    assert_ne!(before_unbonding_delay, new_unbonding_delay);

    assert_eq!(
        new_unbonding_delay, after_unbonding_delay,
        "Should have upgraded locked funds period"
    );
}

#[ignore]
#[test]
fn should_apply_global_state_upgrade() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    // We'll try writing directly to this key.
    let unbonding_delay_key = *builder
        .get_named_keys(EntityAddr::System(
            builder.get_auction_contract_hash().value(),
        ))
        .get(UNBONDING_DELAY_KEY)
        .unwrap();

    let before_unbonding_delay: u64 = builder
        .query(None, unbonding_delay_key, &[])
        .expect("should have locked funds period")
        .as_cl_value()
        .expect("should be a CLValue")
        .clone()
        .into_t()
        .expect("should be u64");

    let new_unbonding_delay = DEFAULT_UNBONDING_DELAY + 5;

    let mut update_map = BTreeMap::new();
    update_map.insert(
        unbonding_delay_key,
        StoredValue::from(CLValue::from_t(new_unbonding_delay).expect("should create a CLValue")),
    );

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_global_state_update(update_map)
            .build()
    };

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    let after_unbonding_delay: u64 = builder
        .query(None, unbonding_delay_key, &[])
        .expect("should have locked funds period")
        .as_cl_value()
        .expect("should be a CLValue")
        .clone()
        .into_t()
        .expect("should be u64");

    assert_ne!(before_unbonding_delay, new_unbonding_delay);

    assert_eq!(
        new_unbonding_delay, after_unbonding_delay,
        "Should have modified locked funds period"
    );
}

#[ignore]
#[test]
fn should_increase_max_associated_keys_after_upgrade() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .build()
    };

    let enable_entity = false;
    let max_associated_keys = DEFAULT_MAX_ASSOCIATED_KEYS + 1;
    let core_config = CoreConfig {
        max_associated_keys,
        enable_addressable_entity: enable_entity,
        ..Default::default()
    };

    let chainspec = ChainspecConfig {
        core_config,
        wasm_config: Default::default(),
        system_costs_config: Default::default(),
        storage_costs: StorageCosts::default(),
    };
    builder.with_chainspec(chainspec);

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    for n in (0..DEFAULT_MAX_ASSOCIATED_KEYS).map(U256::from) {
        let account_hash = {
            let mut addr = [0; ACCOUNT_HASH_LENGTH];
            n.to_big_endian(&mut addr);
            AccountHash::new(addr)
        };

        let add_request = ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            "add_update_associated_key.wasm",
            runtime_args! {
                ARG_ACCOUNT => account_hash,
            },
        )
        .build();

        builder.exec(add_request).expect_success().commit();
    }

    let account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account");

    assert!(account.associated_keys().len() > DEFAULT_MAX_ASSOCIATED_KEYS as usize);
    assert_eq!(
        account.associated_keys().len(),
        max_associated_keys as usize
    );
}

#[ignore]
#[test]
fn should_correctly_migrate_and_prune_system_contract_records() {
    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_3_1);

    let legacy_system_entity_registry = {
        let stored_value: StoredValue = builder
            .query(None, ENTRY_REGISTRY_SPECIAL_ADDRESS, &[])
            .expect("should query system entity registry");
        let cl_value = stored_value
            .as_cl_value()
            .cloned()
            .expect("should have cl value");
        let registry: SystemHashRegistry = cl_value.into_t().expect("should have system registry");
        registry
    };

    let old_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let mut global_state_update = BTreeMap::<Key, StoredValue>::new();

    let registry = CLValue::from_t(legacy_system_entity_registry.clone())
        .expect("must convert to StoredValue")
        .into();

    global_state_update.insert(Key::SystemEntityRegistry, registry);

    let mut upgrade_request = UpgradeRequestBuilder::new()
        .with_current_protocol_version(old_protocol_version)
        .with_new_protocol_version(ProtocolVersion::from_parts(2, 0, 0))
        .with_activation_point(DEFAULT_ACTIVATION_POINT)
        .with_global_state_update(global_state_update)
        .build();

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    let system_names = vec![system::MINT, system::AUCTION, system::HANDLE_PAYMENT];

    for name in system_names {
        let legacy_hash = *legacy_system_entity_registry
            .get(name)
            .expect("must have hash");
        let legacy_contract_key = Key::Hash(legacy_hash);
        let _legacy_query = builder.query(None, legacy_contract_key, &[]);

        builder
            .get_addressable_entity(AddressableEntityHash::new(legacy_hash))
            .expect("must have system entity");
    }
}

#[test]
fn should_not_migrate_bids_with_invalid_min_max_delegation_amounts() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_maximum_delegation_amount(250_000_000_000)
            .with_minimum_delegation_amount(500_000_000_000)
            .build()
    };

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_failure();
}

#[test]
fn should_upgrade_legacy_accounts() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let mut rng = rand::thread_rng();
    let account_data = (0..10000).map(|_| {
        let account_hash = rng.gen();
        let main_purse_uref = rng.gen();

        let account_key = Key::Account(account_hash);
        let account_value = StoredValue::Account(Account::create(
            account_hash,
            NamedKeys::new(),
            main_purse_uref,
        ));

        (account_key, account_value)
    });

    builder.write_data_and_commit(account_data);

    let sem_ver = PROTOCOL_VERSION.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(PROTOCOL_VERSION)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_minimum_delegation_amount(250_000_000_000)
            .with_maximum_delegation_amount(500_000_000_000)
            .build()
    };

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();
}

#[ignore]
#[test]
fn should_migrate_seigniorage_snapshot_to_new_version() {
    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_4_3);

    let auction_contract_hash = builder.get_auction_contract_hash();

    // get legacy auction contract
    let auction_contract = builder
        .query(None, Key::Hash(auction_contract_hash.value()), &[])
        .expect("should have auction contract")
        .into_contract()
        .expect("should have legacy Contract under the Key::Contract variant");

    // check that snapshot version key does not exist yet
    let auction_named_keys = auction_contract.named_keys();
    let maybe_snapshot_version_named_key =
        auction_named_keys.get(SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION_KEY);
    assert!(maybe_snapshot_version_named_key.is_none());

    // fetch legacy snapshot
    let legacy_seigniorage_snapshot: SeigniorageRecipientsSnapshotV1 = {
        let snapshot_key = auction_named_keys
            .get(SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY)
            .expect("snapshot named key should exist");
        builder
            .query(None, *snapshot_key, &[])
            .expect("should have seigniorage snapshot")
            .as_cl_value()
            .expect("should be a CLValue")
            .clone()
            .into_t()
            .expect("should be SeigniorageRecipientsSnapshotV1")
    };

    // prepare upgrade request
    let old_protocol_version = lmdb_fixture_state.genesis_protocol_version();
    let mut upgrade_request = UpgradeRequestBuilder::new()
        .with_current_protocol_version(old_protocol_version)
        .with_new_protocol_version(ProtocolVersion::from_parts(2, 0, 0))
        .with_activation_point(DEFAULT_ACTIVATION_POINT)
        .build();

    // execute upgrade
    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    // fetch updated named keys
    let auction_named_keys =
        builder.get_named_keys(EntityAddr::System(auction_contract_hash.value()));

    // check that snapshot version named key was populated
    let snapshot_version_key = auction_named_keys
        .get(SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION_KEY)
        .expect("auction should have snapshot version named key");
    let snapshot_version: u8 = builder
        .query(None, *snapshot_version_key, &[])
        .expect("should have seigniorage snapshot version")
        .as_cl_value()
        .expect("should be a CLValue")
        .clone()
        .into_t()
        .expect("should be u8");
    assert_eq!(
        snapshot_version,
        DEFAULT_SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION
    );

    // fetch new snapshot
    let seigniorage_snapshot: SeigniorageRecipientsSnapshotV2 = {
        let snapshot_key = auction_named_keys
            .get(SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY)
            .expect("snapshot named key should exist");
        builder
            .query(None, *snapshot_key, &[])
            .expect("should have seigniorage snapshot")
            .as_cl_value()
            .expect("should be a CLValue")
            .clone()
            .into_t()
            .expect("should be SeigniorageRecipientsSnapshotV2")
    };

    // compare snapshots
    for era_id in legacy_seigniorage_snapshot.keys() {
        let legacy_seigniorage_recipients = legacy_seigniorage_snapshot.get(era_id).unwrap();
        let new_seigniorage_recipient = seigniorage_snapshot.get(era_id).unwrap();

        for pubkey in legacy_seigniorage_recipients.keys() {
            let legacy_recipient = legacy_seigniorage_recipients.get(pubkey).unwrap();
            let new_recipient = new_seigniorage_recipient.get(pubkey).unwrap();

            assert_eq!(legacy_recipient.stake(), new_recipient.stake());
            assert_eq!(
                legacy_recipient.delegation_rate(),
                new_recipient.delegation_rate()
            );
            for pk in legacy_recipient.delegator_stake().keys() {
                assert!(new_recipient
                    .delegator_stake()
                    .contains_key(&DelegatorKind::PublicKey(pk.clone())))
            }
        }
    }
}
