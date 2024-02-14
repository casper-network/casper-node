use once_cell::sync::Lazy;
use rand::Rng;
use tempfile::TempDir;

use casper_engine_test_support::{
    LmdbWasmTestBuilder, UpgradeRequestBuilder, DEFAULT_EXEC_CONFIG, DEFAULT_GENESIS_CONFIG_HASH,
    DEFAULT_PROTOCOL_VERSION, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::EngineConfig;
use casper_storage::data_access_layer::GenesisRequest;
use casper_types::{ChainspecRegistry, Digest, EraId, Key, ProtocolVersion};

use crate::lmdb_fixture;

const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);

static OLD_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| *DEFAULT_PROTOCOL_VERSION);
static NEW_PROTOCOL_VERSION: Lazy<ProtocolVersion> = Lazy::new(|| {
    ProtocolVersion::from_parts(
        OLD_PROTOCOL_VERSION.value().major,
        OLD_PROTOCOL_VERSION.value().minor,
        OLD_PROTOCOL_VERSION.value().patch + 1,
    )
});

#[ignore]
#[test]
fn should_commit_chainspec_registry_during_genesis() {
    let mut rng = rand::thread_rng();
    let chainspec_bytes = rng.gen::<[u8; 32]>();
    let genesis_account = rng.gen::<[u8; 32]>();
    let chainspec_bytes_hash = Digest::hash(chainspec_bytes);
    let genesis_account_hash = Digest::hash(genesis_account);

    let chainspec_registry =
        ChainspecRegistry::new_with_genesis(&chainspec_bytes, &genesis_account);

    let run_genesis_request = GenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        DEFAULT_EXEC_CONFIG.clone(),
        chainspec_registry,
    );

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(run_genesis_request);

    let queried_registry = builder
        .query(None, Key::ChainspecRegistry, &[])
        .expect("must have entry under Key::ChainspecRegistry")
        .as_cl_value()
        .expect("must have underlying cl_value")
        .to_owned()
        .into_t::<ChainspecRegistry>()
        .expect("must convert to chainspec registry");

    let queried_chainspec_hash = queried_registry.chainspec_raw_hash();
    assert_eq!(*queried_chainspec_hash, chainspec_bytes_hash);

    let queried_accounts_hash = queried_registry
        .genesis_accounts_raw_hash()
        .expect("must have entry for genesis accounts");
    assert_eq!(*queried_accounts_hash, genesis_account_hash);
}

#[ignore]
#[test]
#[should_panic]
fn should_fail_to_commit_genesis_when_missing_genesis_accounts_hash() {
    let mut rng = rand::thread_rng();
    let chainspec_bytes = rng.gen::<[u8; 32]>();

    let incomplete_chainspec_registry =
        ChainspecRegistry::new_with_optional_global_state(&chainspec_bytes, None);

    let run_genesis_request = GenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        DEFAULT_EXEC_CONFIG.clone(),
        incomplete_chainspec_registry,
    );

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(run_genesis_request);
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
struct TestConfig {
    with_global_state_bytes: bool,
    from_v1_4_4: bool,
}

fn should_upgrade_chainspec_registry(cfg: TestConfig) {
    let mut rng = rand::thread_rng();
    let data_dir = TempDir::new().expect("should create temp dir");

    let mut builder = if cfg.from_v1_4_4 {
        let (builder, _lmdb_fixture_state, _temp_dir) =
            lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_4_4);
        builder
    } else {
        let mut builder = LmdbWasmTestBuilder::new(data_dir.path());
        builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());
        builder
    };

    let chainspec_bytes = rng.gen::<[u8; 32]>();
    let global_state_bytes = rng.gen::<[u8; 32]>();
    let chainspec_bytes_hash = Digest::hash(chainspec_bytes);
    let global_state_bytes_hash = Digest::hash(global_state_bytes);

    let upgraded_chainspec_registry = ChainspecRegistry::new_with_optional_global_state(
        &chainspec_bytes,
        cfg.with_global_state_bytes
            .then_some(global_state_bytes.as_slice()),
    );

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(*OLD_PROTOCOL_VERSION)
            .with_new_protocol_version(*NEW_PROTOCOL_VERSION)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_chainspec_registry(upgraded_chainspec_registry)
            .build()
    };

    let engine_config = EngineConfig::default();

    builder
        .upgrade_with_upgrade_request(engine_config, &mut upgrade_request)
        .expect_upgrade_success();

    let queried_registry = builder
        .query(None, Key::ChainspecRegistry, &[])
        .expect("must have entry under Key::ChainspecRegistry")
        .as_cl_value()
        .expect("must have underlying cl_value")
        .to_owned()
        .into_t::<ChainspecRegistry>()
        .expect("must convert to chainspec registry");

    // There should be no entry for the genesis accounts once the upgrade has completed.
    assert!(queried_registry.genesis_accounts_raw_hash().is_none());

    let queried_chainspec_hash = queried_registry.chainspec_raw_hash();
    assert_eq!(*queried_chainspec_hash, chainspec_bytes_hash);

    if cfg.with_global_state_bytes {
        let queried_global_state_toml_hash = queried_registry.global_state_raw_hash().unwrap();
        assert_eq!(*queried_global_state_toml_hash, global_state_bytes_hash);
    } else {
        assert!(queried_registry.global_state_raw_hash().is_none());
    }
}

#[ignore]
#[test]
fn should_upgrade_chainspec_registry_with_global_state_hash() {
    let cfg = TestConfig {
        with_global_state_bytes: true,
        from_v1_4_4: false,
    };
    should_upgrade_chainspec_registry(cfg)
}

#[ignore]
#[test]
fn should_upgrade_chainspec_registry_without_global_state_hash() {
    let cfg = TestConfig {
        with_global_state_bytes: false,
        from_v1_4_4: false,
    };
    should_upgrade_chainspec_registry(cfg)
}

#[ignore]
#[test]
fn should_upgrade_chainspec_registry_with_global_state_hash_from_v1_4_4() {
    let cfg = TestConfig {
        with_global_state_bytes: true,
        from_v1_4_4: true,
    };
    should_upgrade_chainspec_registry(cfg)
}

#[ignore]
#[test]
fn should_upgrade_chainspec_registry_without_global_state_hash_from_v1_4_4() {
    let cfg = TestConfig {
        with_global_state_bytes: false,
        from_v1_4_4: true,
    };
    should_upgrade_chainspec_registry(cfg)
}
