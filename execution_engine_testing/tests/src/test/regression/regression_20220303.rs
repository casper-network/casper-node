use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use casper_engine_test_support::{LmdbWasmTestBuilder, UpgradeRequestBuilder};
use casper_types::{
    contracts::ContractHash,
    system::{self, mint},
    AccessRights, CLValue, Digest, EntityAddr, EntryPoints, EraId, Key, ProtocolVersion,
    StoredValue, SystemHashRegistry, URef,
};
use rand::Rng;

use crate::lmdb_fixture::{self, ENTRY_REGISTRY_SPECIAL_ADDRESS};

const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);

#[ignore]
#[test]
fn should_update_contract_metadata_at_upgrade_with_major_bump() {
    test_upgrade(1, 0, 0, 0);
}

#[ignore]
#[test]
fn should_update_contract_metadata_at_upgrade_with_minor_bump() {
    test_upgrade(0, 1, 0, 0);
}

#[ignore]
#[test]
fn should_update_contract_metadata_at_upgrade_with_patch_bump() {
    test_upgrade(0, 0, 1, 0);
}

#[ignore]
#[test]
fn test_upgrade_with_global_state_update_entries() {
    test_upgrade(0, 0, 1, 20000);
}

fn test_upgrade(major_bump: u32, minor_bump: u32, patch_bump: u32, upgrade_entries: u32) {
    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_3_1);
    let mint_contract_hash = {
        let stored_value: StoredValue = builder
            .query(None, ENTRY_REGISTRY_SPECIAL_ADDRESS, &[])
            .expect("should query system entity registry");
        let cl_value = stored_value
            .as_cl_value()
            .cloned()
            .expect("should have cl value");
        let registry: SystemHashRegistry = cl_value.into_t().expect("should have system registry");
        registry
            .get(system::MINT)
            .cloned()
            .expect("should contract hash")
    };
    let old_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let legacy_mint_hash = ContractHash::new(mint_contract_hash);

    let old_mint_contract = builder
        .get_contract(legacy_mint_hash)
        .expect("should have mint contract");
    assert_eq!(old_mint_contract.protocol_version(), old_protocol_version);
    let new_protocol_version = ProtocolVersion::from_parts(
        old_protocol_version.value().major + major_bump,
        old_protocol_version.value().minor + minor_bump,
        old_protocol_version.value().patch + patch_bump,
    );
    let mut global_state_update =
        apply_global_state_update(&builder, lmdb_fixture_state.post_state_hash);

    let mut rng = casper_types::testing::TestRng::new();
    if upgrade_entries > 0 {
        for _ in 0..upgrade_entries {
            global_state_update.insert(
                Key::URef(URef::new(rng.gen(), AccessRights::empty())),
                StoredValue::CLValue(CLValue::from_t(rng.gen::<u64>()).unwrap()),
            );
        }
    }

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(old_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(DEFAULT_ACTIVATION_POINT)
            .with_global_state_update(global_state_update)
            .build()
    };
    let start = Instant::now();
    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(40),
        "upgrade took too long! {} (millis)",
        elapsed.as_millis()
    );
    let new_contract = builder
        .get_addressable_entity(mint_contract_hash.into())
        .expect("should have mint contract");
    assert_eq!(
        old_mint_contract.contract_package_hash().value(),
        new_contract.package_hash().value()
    );
    assert_eq!(
        old_mint_contract.contract_wasm_hash().value(),
        new_contract.byte_code_hash().value()
    );
    let new_entry_points = builder.get_entry_points(EntityAddr::System(mint_contract_hash));
    let old_entry_points = EntryPoints::from(old_mint_contract.entry_points().clone());
    assert_ne!(&old_entry_points, &new_entry_points);
    assert_eq!(
        &new_entry_points,
        &mint::mint_entry_points(),
        "should have new entrypoints written"
    );
    assert_eq!(new_contract.protocol_version(), new_protocol_version);
}

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
        .into_t::<SystemHashRegistry>()
        .expect("must convert to btree map");

    let mut global_state_update = BTreeMap::<Key, StoredValue>::new();
    let registry = CLValue::from_t(system_contract_hashes)
        .expect("must convert to StoredValue")
        .into();

    global_state_update.insert(Key::SystemEntityRegistry, registry);

    global_state_update
}
