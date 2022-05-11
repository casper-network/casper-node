use std::ops::RangeInclusive;

use proptest::{
    array,
    collection::vec,
    prelude::{any, proptest, Strategy},
};

use super::*;

const DEFAULT_MIN_LENGTH: usize = 0;

const DEFAULT_MAX_LENGTH: usize = 100;

fn get_range() -> RangeInclusive<usize> {
    let start = option_env!("CL_TRIE_TEST_VECTOR_MIN_LENGTH")
        .and_then(|s| str::parse::<usize>(s).ok())
        .unwrap_or(DEFAULT_MIN_LENGTH);
    let end = option_env!("CL_TRIE_TEST_VECTOR_MAX_LENGTH")
        .and_then(|s| str::parse::<usize>(s).ok())
        .unwrap_or(DEFAULT_MAX_LENGTH);
    RangeInclusive::new(start, end)
}

fn lmdb_roundtrip_succeeds(pairs: &[(TestKey, TestValue)]) -> bool {
    let correlation_id = CorrelationId::new();
    let (root_hash, tries) = TEST_TRIE_GENERATORS[0]().unwrap();
    let context = RocksDbTestContext::new(&tries).unwrap();
    let mut states_to_check = vec![];

    let root_hashes =
        write_pairs::<_, _, _, error::Error>(correlation_id, &context.store, &root_hash, pairs)
            .unwrap();

    states_to_check.extend(root_hashes);

    check_pairs::<_, _, _, error::Error>(correlation_id, &context.store, &states_to_check, pairs)
        .unwrap();

    check_pairs_proofs::<_, _, _, error::Error>(
        correlation_id,
        &context.store,
        &states_to_check,
        pairs,
    )
    .unwrap()
}

fn in_memory_roundtrip_succeeds(pairs: &[(TestKey, TestValue)]) -> bool {
    let correlation_id = CorrelationId::new();
    let (root_hash, tries) = TEST_TRIE_GENERATORS[0]().unwrap();
    let context = InMemoryTestContext::new(&tries).unwrap();
    let mut states_to_check = vec![];

    let root_hashes =
        write_pairs::<_, _, _, in_memory::Error>(correlation_id, &context.store, &root_hash, pairs)
            .unwrap();

    states_to_check.extend(root_hashes);

    check_pairs::<_, _, _, in_memory::Error>(
        correlation_id,
        &context.store,
        &states_to_check,
        pairs,
    )
    .unwrap();

    check_pairs_proofs::<_, _, _, in_memory::Error>(
        correlation_id,
        &context.store,
        &states_to_check,
        pairs,
    )
    .unwrap()
}

fn test_key_arb() -> impl Strategy<Value = TestKey> {
    array::uniform7(any::<u8>()).prop_map(TestKey)
}

fn test_value_arb() -> impl Strategy<Value = TestValue> {
    array::uniform6(any::<u8>()).prop_map(TestValue)
}

proptest! {
    #[test]
    fn prop_in_memory_roundtrip_succeeds(inputs in vec((test_key_arb(), test_value_arb()), get_range())) {
        assert!(in_memory_roundtrip_succeeds(&inputs));
    }

    #[test]
    fn prop_lmdb_roundtrip_succeeds(inputs in vec((test_key_arb(), test_value_arb()), get_range())) {
        assert!(lmdb_roundtrip_succeeds(&inputs));
    }
}
