use std::sync::Arc;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_storage::global_state::{
    state::{lmdb::LmdbGlobalState, StateProvider},
    transaction_source::lmdb::LmdbEnvironment,
    trie_store::lmdb::LmdbTrieStore,
};
use casper_types::RuntimeArgs;
use lmdb::DatabaseFlags;

const CONTRACT_DO_NOTHING: &str = "do_nothing.wasm";

#[ignore]
#[test]
fn regression_test_genesis_hash_mismatch() {
    let mut builder_base = LmdbWasmTestBuilder::default();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_DO_NOTHING,
        RuntimeArgs::default(),
    )
    .build();

    // Step 1.
    let builder = builder_base.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // This is trie's post state hash after calling run_genesis endpoint.
    // Step 1a)
    let genesis_run_hash = builder.get_genesis_hash();
    let genesis_transforms = builder.get_genesis_effects().clone();

    let empty_root_hash = {
        let gs = {
            let tempdir = tempfile::tempdir().expect("should create tempdir");
            let lmdb_environment = LmdbEnvironment::new(tempdir.path(), 1024 * 1024, 32, false)
                .expect("should create lmdb environment");
            let lmdb_trie_store =
                LmdbTrieStore::new(&lmdb_environment, None, DatabaseFlags::default())
                    .expect("should create lmdb trie store");

            LmdbGlobalState::empty(Arc::new(lmdb_environment), Arc::new(lmdb_trie_store), 6)
                .expect("Empty GlobalState.")
        };
        gs.empty_root()
    };

    // This is trie's post state hash after committing genesis effects on top of
    // empty trie. Step 1b)
    let genesis_transforms_hash = builder
        .commit_transforms(empty_root_hash, genesis_transforms)
        .get_post_state_hash();

    // They should match.
    assert_eq!(genesis_run_hash, genesis_transforms_hash);

    // Step 2.
    builder.exec(exec_request_1).commit().expect_success();

    // No step 3.
    // Step 4.
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    // Step 4a)
    let second_genesis_run_hash = builder.get_genesis_hash();
    let second_genesis_transforms = builder.get_genesis_effects().clone();

    // Step 4b)
    let second_genesis_transforms_hash = builder
        .commit_transforms(empty_root_hash, second_genesis_transforms)
        .get_post_state_hash();

    assert_eq!(second_genesis_run_hash, second_genesis_transforms_hash);

    assert_eq!(second_genesis_run_hash, genesis_run_hash);
    assert_eq!(second_genesis_transforms_hash, genesis_transforms_hash);
}
