use std::{collections::BTreeMap, ops::RangeInclusive};

use lmdb::DatabaseFlags;
use proptest::{collection::vec, prelude::proptest};
use tempfile::tempdir;

use types::{bytesrepr::ToBytes, Key};

use crate::{
    components::contract_runtime::{
        shared::{newtypes::Blake2bHash, stored_value::StoredValue},
        storage::{
            store::tests as store_tests,
            trie::{gens::trie_arb, Trie},
            TEST_MAP_SIZE,
        },
    },
    crypto::hash,
};

const DEFAULT_MIN_LENGTH: usize = 1;
const DEFAULT_MAX_LENGTH: usize = 4;

fn get_range() -> RangeInclusive<usize> {
    let start = option_env!("CL_TRIE_STORE_TEST_VECTOR_MIN_LENGTH")
        .and_then(|s| str::parse::<usize>(s).ok())
        .unwrap_or(DEFAULT_MIN_LENGTH);
    let end = option_env!("CL_TRIE_STORE_TEST_VECTOR_MAX_LENGTH")
        .and_then(|s| str::parse::<usize>(s).ok())
        .unwrap_or(DEFAULT_MAX_LENGTH);
    RangeInclusive::new(start, end)
}

fn in_memory_roundtrip_succeeds(inputs: Vec<Trie<Key, StoredValue>>) -> bool {
    use crate::components::contract_runtime::storage::{
        transaction_source::in_memory::InMemoryEnvironment,
        trie_store::in_memory::InMemoryTrieStore,
    };

    let env = InMemoryEnvironment::new();
    let store = InMemoryTrieStore::new(&env, None);

    let inputs: BTreeMap<Blake2bHash, Trie<Key, StoredValue>> = inputs
        .into_iter()
        .map(|trie| (hash::hash(&trie.to_bytes().unwrap()), trie))
        .collect();

    store_tests::roundtrip_succeeds(&env, &store, inputs).unwrap()
}

fn lmdb_roundtrip_succeeds(inputs: Vec<Trie<Key, StoredValue>>) -> bool {
    use crate::components::contract_runtime::storage::{
        transaction_source::lmdb::LmdbEnvironment, trie_store::lmdb::LmdbTrieStore,
    };

    let tmp_dir = tempdir().unwrap();
    let env = LmdbEnvironment::new(&tmp_dir.path().to_path_buf(), *TEST_MAP_SIZE).unwrap();
    let store = LmdbTrieStore::new(&env, None, DatabaseFlags::empty()).unwrap();

    let inputs: BTreeMap<Blake2bHash, Trie<Key, StoredValue>> = inputs
        .into_iter()
        .map(|trie| (hash::hash(&trie.to_bytes().unwrap()), trie))
        .collect();

    let ret = store_tests::roundtrip_succeeds(&env, &store, inputs).unwrap();
    tmp_dir.close().unwrap();
    ret
}

proptest! {
    #[test]
    fn prop_in_memory_roundtrip_succeeds(v in vec(trie_arb(), get_range())) {
        assert!(in_memory_roundtrip_succeeds(v))
    }

    #[test]
    fn prop_lmdb_roundtrip_succeeds(v in vec(trie_arb(), get_range())) {
        assert!(lmdb_roundtrip_succeeds(v))
    }
}
