use casper_types::Digest;
use convert::TryInto;

use super::*;
use crate::global_state::{
    error,
    trie::LazilyDeserializedTrie,
    trie_store::operations::{scan_raw, store_wrappers, TrieScanRaw},
};

fn check_scan<'a, R, S, E>(
    environment: &'a R,
    store: &S,
    root_hash: &Digest,
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
    let root_bytes = root.to_bytes()?;
    let store = store_wrappers::NonDeserializingStore::new(store);
    let TrieScanRaw { mut tip, parents } = scan_raw::<TestKey, TestValue, R::ReadTransaction, S, E>(
        &txn,
        &store,
        key,
        root_bytes.into(),
    )?;

    for (index, parent) in parents.into_iter().rev() {
        let expected_tip_hash = {
            match tip {
                LazilyDeserializedTrie::Leaf(leaf_bytes) => Digest::hash(leaf_bytes.bytes()),
                node @ LazilyDeserializedTrie::Node { .. }
                | node @ LazilyDeserializedTrie::Extension { .. } => {
                    let tip_bytes = TryInto::<Trie<TestKey, TestValue>>::try_into(node)?
                        .to_bytes()
                        .unwrap();
                    Digest::hash(&tip_bytes)
                }
            }
        };
        match parent {
            Trie::Leaf { .. } => panic!("parents should not contain any leaves"),
            Trie::Node { pointer_block } => {
                let pointer_tip_hash = pointer_block[<usize>::from(index)].map(|ptr| *ptr.hash());
                assert_eq!(Some(expected_tip_hash), pointer_tip_hash);
                tip = LazilyDeserializedTrie::Node { pointer_block };
            }
            Trie::Extension { affix, pointer } => {
                let pointer_tip_hash = pointer.hash().to_owned();
                assert_eq!(expected_tip_hash, pointer_tip_hash);
                tip = LazilyDeserializedTrie::Extension { affix, pointer };
            }
        }
    }

    assert!(
        matches!(
            tip,
            LazilyDeserializedTrie::Node { .. } | LazilyDeserializedTrie::Extension { .. },
        ),
        "Unexpected leaf found"
    );
    assert_eq!(root, tip.try_into()?);
    txn.commit()?;
    Ok(())
}

mod partial_tries {
    use super::*;

    #[test]
    fn lmdb_scans_from_n_leaf_partial_trie_had_expected_results() {
        for generator in &TEST_TRIE_GENERATORS {
            let (root_hash, tries) = generator().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();

            for leaf in TEST_LEAVES.iter() {
                let leaf_bytes = leaf.to_bytes().unwrap();
                check_scan::<_, _, error::Error>(
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
        let context = LmdbTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for (state_index, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            for state in &states[..state_index] {
                for leaf in TEST_LEAVES.iter() {
                    let leaf_bytes = leaf.to_bytes().unwrap();
                    check_scan::<_, _, error::Error>(
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
