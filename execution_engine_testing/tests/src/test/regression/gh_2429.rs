use std::collections::BTreeMap;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, UpgradeRequestBuilder, DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::{
    core::engine_state::SystemContractRegistry, shared::newtypes::CorrelationId,
};
use casper_hashing::Digest;
use casper_types::{
    runtime_args, system, AccessRights, CLValue, EraId, Key, ProtocolVersion, RuntimeArgs,
    StoredValue, URef,
};
use tempfile::TempDir;

use crate::lmdb_fixture::{self, LmdbFixtureState};

const DEFAULT_ACTIVATION_POINT: EraId = EraId::new(1);

#[ignore]
#[test]
fn gh_2429_sparse_serialization() {
    let (mut builder, _lmdb_fixture_state, protocol_version, _temp_dir) = setup();

    let stored_value = builder
        .query_with_proof(None, Key::SystemContractRegistry, &[])
        .unwrap();

    let _trie = builder
        .get_engine_state()
        .get_trie(CorrelationId::default(), builder.get_post_state_hash())
        .unwrap();

    let system_contracts: SystemContractRegistry = stored_value
        .as_cl_value()
        .cloned()
        .unwrap()
        .into_t()
        .unwrap();

    let _mint = system_contracts
        .get(system::MINT)
        .expect("should have mint system contract");

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        "create_purse_01.wasm",
        runtime_args! {
            "purse_name" => "test_purse",
        },
    )
    .with_protocol_version(protocol_version)
    .build();

    builder.exec(exec_request).expect_success().commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");
    let _purse_key = account.named_keys()["test_purse"]
        .into_uref()
        .expect("should have purse uref");
}

fn setup() -> (
    LmdbWasmTestBuilder,
    LmdbFixtureState,
    ProtocolVersion,
    TempDir,
) {
    let (mut builder, lmdb_fixture_state, temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture(lmdb_fixture::RELEASE_1_3_1);

    let global_state_update =
        apply_global_state_update(&builder, lmdb_fixture_state.post_state_hash);

    let previous_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let current_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let new_protocol_version =
        ProtocolVersion::from_parts(current_protocol_version.value().major + 1, 0, 0);

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

    (builder, lmdb_fixture_state, new_protocol_version, temp_dir)
}

fn apply_global_state_update(
    builder: &LmdbWasmTestBuilder,
    post_state_hash: Digest,
) -> BTreeMap<Key, StoredValue> {
    let key = URef::new([0u8; 32], AccessRights::all()).into();

    let stored_value = builder
        .query_with_proof(Some(post_state_hash), key, &Vec::new())
        .expect("Must have stored system contract hashes");

    let system_contract_hashes = stored_value
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
