use crate::shared::newtypes::{Blake2bHash, CorrelationId};

use super::*;
use crate::storage::{
    error::{self, in_memory},
    trie_store::operations::{scan, TrieScan},
};

fn check_scan<'a, R, S, E>(
    correlation_id: CorrelationId,
    environment: &'a R,
    store: &S,
    root_hash: &Blake2bHash,
    key: &[u8],
) -> Result<(), E>
where
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<TestKey, TestValue>,
    S::Error: From<R::Error> + std::fmt::Debug,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
{
    let txn: R::ReadTransaction = environment.create_read_txn()?;
    let root = store
        .get(&txn, root_hash)?
        .expect("check_scan received an invalid root hash");
    let TrieScan { mut tip, parents } = scan::<TestKey, TestValue, R::ReadTransaction, S, E>(
        correlation_id,
        &txn,
        store,
        key,
        &root,
    )?;

    for (index, parent) in parents.into_iter().rev() {
        let expected_tip_hash = {
            let tip_bytes = tip.to_bytes().unwrap();
            Blake2bHash::new(&tip_bytes)
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
    txn.commit()?;
    Ok(())
}

mod partial_tries {
    use super::*;

    #[test]
    fn lmdb_scans_from_n_leaf_partial_trie_had_expected_results() {
        for generator in &TEST_TRIE_GENERATORS {
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = generator().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();

            for leaf in TEST_LEAVES.iter() {
                let leaf_bytes = leaf.to_bytes().unwrap();
                check_scan::<_, _, error::Error>(
                    correlation_id,
                    &context.environment,
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
                check_scan::<_, _, in_memory::Error>(
                    correlation_id,
                    &context.environment,
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
        let context = LmdbTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Blake2bHash> = Vec::new();

        for (state_index, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            for state in &states[..state_index] {
                for leaf in TEST_LEAVES.iter() {
                    let leaf_bytes = leaf.to_bytes().unwrap();
                    check_scan::<_, _, error::Error>(
                        correlation_id,
                        &context.environment,
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
        let mut states: Vec<Blake2bHash> = Vec::new();

        for (state_index, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            for state in &states[..state_index] {
                for leaf in TEST_LEAVES.iter() {
                    let leaf_bytes = leaf.to_bytes().unwrap();
                    check_scan::<_, _, in_memory::Error>(
                        correlation_id,
                        &context.environment,
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
