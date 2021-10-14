use std::{collections::BTreeMap, ops::RangeInclusive};

use lmdb::DatabaseFlags;
use proptest::{collection, prelude::proptest};

use casper_types::{gens as gens_ext, ProtocolVersion};

use crate::storage::{
    protocol_data::{gens, ProtocolData},
    protocol_data_store::{in_memory::InMemoryProtocolDataStore, lmdb::LmdbProtocolDataStore},
    store::tests as store_tests,
    transaction_source::{in_memory::InMemoryEnvironment, lmdb::LmdbEnvironment},
    DEFAULT_TEST_MAX_DB_SIZE, DEFAULT_TEST_MAX_READERS,
};

const DEFAULT_MIN_LENGTH: usize = 1;
const DEFAULT_MAX_LENGTH: usize = 16;

fn get_range() -> RangeInclusive<usize> {
    let start = option_env!("CL_PROTOCOL_DATA_STORE_TEST_MAP_MIN_LENGTH")
        .and_then(|s| str::parse::<usize>(s).ok())
        .unwrap_or(DEFAULT_MIN_LENGTH);
    let end = option_env!("CL_PROTOCOL_DATA_STORE_TEST_MAP_MAX_LENGTH")
        .and_then(|s| str::parse::<usize>(s).ok())
        .unwrap_or(DEFAULT_MAX_LENGTH);
    RangeInclusive::new(start, end)
}

fn in_memory_roundtrip_succeeds(inputs: BTreeMap<ProtocolVersion, ProtocolData>) -> bool {
    let env = InMemoryEnvironment::new();
    let store = InMemoryProtocolDataStore::new(&env, None);

    store_tests::roundtrip_succeeds(&env, &store, inputs).unwrap()
}

fn lmdb_roundtrip_succeeds(inputs: BTreeMap<ProtocolVersion, ProtocolData>) -> bool {
    let tmp_dir = tempfile::tempdir().unwrap();
    let env = LmdbEnvironment::new(
        &tmp_dir.path().to_path_buf(),
        DEFAULT_TEST_MAX_DB_SIZE,
        DEFAULT_TEST_MAX_READERS,
        true,
    )
    .unwrap();
    let store = LmdbProtocolDataStore::new(&env, None, DatabaseFlags::empty()).unwrap();

    let ret = store_tests::roundtrip_succeeds(&env, &store, inputs).unwrap();
    tmp_dir.close().unwrap();
    ret
}

proptest! {
    #[test]
    fn prop_in_memory_roundtrip_succeeds(
        m in collection::btree_map(gens_ext::protocol_version_arb(), gens::protocol_data_arb(), get_range())
    ) {
        assert!(in_memory_roundtrip_succeeds(m))
    }

    #[test]
    fn prop_lmdb_roundtrip_succeeds(
        m in collection::btree_map(gens_ext::protocol_version_arb(), gens::protocol_data_arb(), get_range())
    ) {
        assert!(lmdb_roundtrip_succeeds(m))
    }
}
