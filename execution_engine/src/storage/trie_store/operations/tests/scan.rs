use casper_hashing::Digest;

use super::*;
use crate::{
    shared::newtypes::CorrelationId,
    storage::trie_store::operations::{scan, TrieScan},
};

fn check_scan<S, E>(
    correlation_id: CorrelationId,
    store: &S,
    root_hash: &Digest,
    key: &[u8],
) -> Result<(), E>
where
    S: TrieStore<TestKey, TestValue>,
    E: From<S::Error> + From<bytesrepr::Error>,
{
    let root = store
        .get(root_hash)?
        .expect("check_scan received an invalid root hash");
    let TrieScan { mut tip, parents } = scan::<_, _, _, E>(correlation_id, store, key, &root)?;

    for (index, parent) in parents.into_iter().rev() {
        let expected_tip_hash = {
            let tip_bytes = tip.to_bytes().unwrap();
            Digest::hash(&tip_bytes)
        };
        match parent {
            Trie::Leaf { .. } => panic!("parents should not contain any leaves"),
            Trie::Node { pointer_block } => {
                let pointer_tip_hash = pointer_block[<usize>::from(index)].map(|ptr| *ptr.hash());
                assert_eq!(Some(expected_tip_hash), pointer_tip_hash);
                tip = Trie::Node { pointer_block };
            }
            Trie::Extension { affix, pointer } => {
                let pointer_tip_hash = pointer.hash().to_owned();
                assert_eq!(expected_tip_hash, pointer_tip_hash);
                tip = Trie::Extension { affix, pointer };
            }
        }
    }
    assert_eq!(root, tip);
    Ok(())
}

mod partial_tries {
    use super::*;

    #[test]
    fn lmdb_scans_from_n_leaf_partial_trie_had_expected_results() {
        for generator in &TEST_TRIE_GENERATORS {
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = generator().unwrap();
            let context = RocksDbTestContext::new(&tries).unwrap();

            for leaf in TEST_LEAVES.iter() {
                let leaf_bytes = leaf.to_bytes().unwrap();
                check_scan::<_, error::Error>(
                    correlation_id,
                    &context.store,
                    &root_hash,
                    &leaf_bytes,
                )
                .unwrap()
            }
        }
    }

    #[test]
    fn in_memory_scans_from_n_leaf_partial_trie_had_expected_results() {
        for generator in &TEST_TRIE_GENERATORS {
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = generator().unwrap();
            let context = InMemoryTestContext::new(&tries).unwrap();

            for leaf in TEST_LEAVES.iter() {
                let leaf_bytes = leaf.to_bytes().unwrap();
                check_scan::<_, in_memory::Error>(
                    correlation_id,
                    &context.store,
                    &root_hash,
                    &leaf_bytes,
                )
                .unwrap()
            }
        }
    }
}

mod full_tries {
    use super::*;

    #[test]
    fn lmdb_scans_from_n_leaf_full_trie_had_expected_results() {
        let correlation_id = CorrelationId::new();
        let context = RocksDbTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for (state_index, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            for state in &states[..state_index] {
                for leaf in TEST_LEAVES.iter() {
                    let leaf_bytes = leaf.to_bytes().unwrap();
                    check_scan::<_, error::Error>(
                        correlation_id,
                        &context.store,
                        state,
                        &leaf_bytes,
                    )
                    .unwrap()
                }
            }
        }
    }

    #[test]
    fn in_memory_scans_from_n_leaf_full_trie_had_expected_results() {
        let correlation_id = CorrelationId::new();
        let context = InMemoryTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for (state_index, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            for state in &states[..state_index] {
                for leaf in TEST_LEAVES.iter() {
                    let leaf_bytes = leaf.to_bytes().unwrap();
                    check_scan::<_, in_memory::Error>(
                        correlation_id,
                        &context.store,
                        state,
                        &leaf_bytes,
                    )
                    .unwrap()
                }
            }
        }
    }
}
