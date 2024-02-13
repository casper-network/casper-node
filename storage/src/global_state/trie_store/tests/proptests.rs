use std::{collections::BTreeMap, ops::RangeInclusive};

use lmdb::DatabaseFlags;
use proptest::{collection::vec, prelude::proptest};
use tempfile::tempdir;

use casper_types::{bytesrepr::ToBytes, Digest, Key, StoredValue};

use crate::global_state::{
    store::tests as store_tests,
    trie::{
        gens::{trie_extension_arb, trie_leaf_arb, trie_node_arb},
        Trie,
    },
    DEFAULT_MAX_DB_SIZE, DEFAULT_MAX_READERS,
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

fn lmdb_roundtrip_succeeds(inputs: Vec<Trie<Key, StoredValue>>) -> bool {
    use crate::global_state::{
        transaction_source::lmdb::LmdbEnvironment, trie_store::lmdb::LmdbTrieStore,
    };

    let tmp_dir = tempdir().unwrap();
    let env = LmdbEnvironment::new(
        tmp_dir.path(),
        DEFAULT_MAX_DB_SIZE,
        DEFAULT_MAX_READERS,
        true,
    )
    .unwrap();
    let store = LmdbTrieStore::new(&env, None, DatabaseFlags::empty()).unwrap();

    let inputs: BTreeMap<Digest, Trie<Key, StoredValue>> = inputs
        .into_iter()
        .map(|trie| (Digest::hash(trie.to_bytes().unwrap()), trie))
        .collect();

    let ret = store_tests::roundtrip_succeeds(&env, &store, inputs).unwrap();
    tmp_dir.close().unwrap();
    ret
}

proptest! {
    #[test]
    fn prop_lmdb_roundtrip_succeeds_leaf(v in vec(trie_leaf_arb(), get_range())) {
        assert!(lmdb_roundtrip_succeeds(v))
    }

    #[test]
    fn prop_lmdb_roundtrip_succeeds_node(v in vec(trie_node_arb(), get_range())) {
        assert!(lmdb_roundtrip_succeeds(v))
    }

    #[test]
    fn prop_lmdb_roundtrip_succeeds_extension(v in vec(trie_extension_arb(), get_range())) {
        assert!(lmdb_roundtrip_succeeds(v))
    }
}
