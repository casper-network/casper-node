use std::collections::BTreeMap;

use casper_engine_test_support::{
    InMemoryWasmTestBuilder, LmdbWasmTestBuilder, UpgradeRequestBuilder,
    DEFAULT_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::engine_state::{EngineConfig, SystemContractRegistry};
use casper_hashing::Digest;
use casper_types::{AccessRights, CLValue, EraId, Key, ProtocolVersion, StoredValue, URef};

use crate::lmdb_fixture;

const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);

fn apply_global_state_update(
    builder: &LmdbWasmTestBuilder,
    post_state_hash: Digest,
) -> BTreeMap<Key, StoredValue> {
    let key = URef::new([0u8; 32], AccessRights::all()).into();

    let system_contract_hashes = builder
        .query(Some(post_state_hash), key, &Vec::new())
        .expect("Must have stored system contract hashes")
        .as_cl_value()
        .expect("must be CLValue")
        .clone()
        .into_t::<SystemContractRegistry>()
        .expect("must convert to btree map");

    let mut global_state_update = BTreeMap::<Key, StoredValue>::new();
    let registry = CLValue::from_t(system_contract_hashes)
        .expect("must convert to StoredValue")
        .into();

    global_state_update.insert(Key::SystemContractRegistry, registry);

    global_state_update
}

#[ignore]
#[test]
fn should_get_auction_info_work_after_genesis() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&*DEFAULT_RUN_GENESIS_REQUEST);
    let _bids = builder.get_bids(None);
    let _era_validators = builder.get_era_validators(None);
}

#[ignore]
#[test]
fn should_get_auction_info_for_older_state_hash() {
    let (mut builder, lmdb_fixture_state, temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_3_1);

    let previous_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let current_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let new_protocol_version =
        ProtocolVersion::from_parts(current_protocol_version.value().major + 1, 0, 0);

    let state_hash_without_registry = builder.get_post_state_hash();

    let global_state_update =
        apply_global_state_update(&builder, lmdb_fixture_state.post_state_hash);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(previous_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_global_state_update(global_state_update)
            .build()
    };

    builder
        .upgrade_with_upgrade_request(*builder.get_engine_state().config(), &mut upgrade_request)
        .expect_upgrade_success();

    let post_upgrade_state_hash = builder.get_post_state_hash();

    let mut builder = LmdbWasmTestBuilder::open(
        &temp_dir.path().join(lmdb_fixture::RELEASE_1_3_1),
        EngineConfig::default(),
        post_upgrade_state_hash,
    );

    // Bug report calls `get-auction-info` which calls `get_bids`, and `get_era_validators`.
    // Both methods are expecting a valid auction contract hash, so we will try to query using a
    // post state hash that is known to not have the `Key::SystemContractHashes`.

    let old_bids_1 = builder.get_bids(Some(state_hash_without_registry));
    let old_bids_2 = builder.get_bids(Some(state_hash_without_registry));
    assert_eq!(old_bids_1, old_bids_2);

    let old_era_validators_1 = builder.get_era_validators(Some(state_hash_without_registry));
    let old_era_validators_2 = builder.get_era_validators(Some(state_hash_without_registry));
    assert_eq!(old_era_validators_1, old_era_validators_2);

    let new_bids = builder.get_bids(None);
    let new_era_validators = builder.get_era_validators(None);

    assert_eq!(old_bids_2, new_bids);
    assert_eq!(old_era_validators_2, new_era_validators);
}
