use std::{borrow::Cow, collections::HashSet};

use num_traits::FromPrimitive;

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    global_state::Pointer,
    Digest,
};

use crate::global_state::{
    error,
    transaction_source::{Readable, Transaction, TransactionSource},
    trie::{Trie, TrieTag},
    trie_store::{
        operations::{
            self,
            tests::{LmdbTestContext, TestKey, TestValue},
            ReadResult,
        },
        TrieStore,
    },
};

/// Given a root hash, find any trie keys that are descendant from it that are referenced but not
/// present in the database.
// TODO: We only need to check one trie key at a time
fn missing_trie_keys<K, V, T, S, E>(
    txn: &T,
    store: &S,
    mut trie_keys_to_visit: Vec<Digest>,
    known_complete: &HashSet<Digest>,
) -> Result<Vec<Digest>, E>
where
    K: ToBytes + FromBytes + Eq + std::fmt::Debug,
    V: ToBytes + FromBytes + std::fmt::Debug,
    T: Readable<Handle = S::Handle>,
    S: TrieStore<K, V>,
    S::Error: From<T::Error>,
    E: From<S::Error> + From<bytesrepr::Error>,
{
    let mut missing_descendants = Vec::new();
    let mut visited = HashSet::new();
    while let Some(trie_key) = trie_keys_to_visit.pop() {
        if !visited.insert(trie_key) {
            continue;
        }

        if known_complete.contains(&trie_key) {
            // Skip because we know there are no missing descendants.
            continue;
        }

        let retrieved_trie_bytes = match store.get_raw(txn, &trie_key)? {
            Some(bytes) => bytes,
            None => {
                // No entry under this trie key.
                missing_descendants.push(trie_key);
                continue;
            }
        };

        // Optimization: Don't deserialize leaves as they have no descendants.
        if let Some(TrieTag::Leaf) = retrieved_trie_bytes
            .first()
            .copied()
            .and_then(TrieTag::from_u8)
        {
            continue;
        }

        // Parse the trie, handling errors gracefully.
        let retrieved_trie = match bytesrepr::deserialize_from_slice(retrieved_trie_bytes) {
            Ok(retrieved_trie) => retrieved_trie,
            // Couldn't parse; treat as missing and continue.
            Err(err) => {
                tracing::error!(?err, "unable to parse trie");
                missing_descendants.push(trie_key);
                continue;
            }
        };

        match retrieved_trie {
            // Should be unreachable due to checking the first byte as a shortcut above.
            Trie::<K, V>::Leaf { .. } => {
                tracing::error!(
                    "did not expect to see a trie leaf in `missing_trie_keys` after shortcut"
                );
            }
            // If we hit a pointer block, queue up all of the nodes it points to
            Trie::Node { pointer_block } => {
                for (_, pointer) in pointer_block.as_indexed_pointers() {
                    match pointer {
                        Pointer::LeafPointer(descendant_leaf_trie_key) => {
                            trie_keys_to_visit.push(descendant_leaf_trie_key)
                        }
                        Pointer::NodePointer(descendant_node_trie_key) => {
                            trie_keys_to_visit.push(descendant_node_trie_key)
                        }
                    }
                }
            }
            // If we hit an extension block, add its pointer to the queue
            Trie::Extension { pointer, .. } => trie_keys_to_visit.push(pointer.into_hash()),
        }
    }
    Ok(missing_descendants)
}

fn copy_state<'a, K, V, R, S, E>(
    source_environment: &'a R,
    source_store: &S,
    target_environment: &'a R,
    target_store: &S,
    root: &Digest,
) -> Result<(), E>
where
    K: ToBytes + FromBytes + Eq + std::fmt::Debug + Copy + Clone + Ord,
    V: ToBytes + FromBytes + Eq + std::fmt::Debug + Copy,
    R: TransactionSource<'a, Handle = S::Handle>,
    S: TrieStore<K, V>,
    S::Error: From<R::Error>,
    E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
{
    // Make sure no missing nodes in source
    {
        let txn: R::ReadTransaction = source_environment.create_read_txn()?;
        let missing_from_source = missing_trie_keys::<_, _, _, _, E>(
            &txn,
            source_store,
            vec![root.to_owned()],
            &Default::default(),
        )?;
        assert_eq!(missing_from_source, Vec::new());
        txn.commit()?;
    }

    // Copy source to target
    {
        let source_txn: R::ReadTransaction = source_environment.create_read_txn()?;
        let mut target_txn: R::ReadWriteTransaction = target_environment.create_read_write_txn()?;
        // Copy source to destination
        let mut queue = vec![root.to_owned()];
        while let Some(trie_key) = queue.pop() {
            let trie_bytes_to_insert = source_store
                .get_raw(&source_txn, &trie_key)?
                .expect("should have trie");
            target_store.put_raw(
                &mut target_txn,
                &trie_key,
                Cow::from(&*trie_bytes_to_insert),
            )?;

            // Now that we've added in `trie_to_insert`, queue up its children
            let new_keys = missing_trie_keys::<_, _, _, _, E>(
                &target_txn,
                target_store,
                vec![trie_key],
                &Default::default(),
            )?;

            queue.extend(new_keys);
        }
        source_txn.commit()?;
        target_txn.commit()?;
    }

    // After the copying process above there should be no missing entries in the target
    {
        let target_txn: R::ReadWriteTransaction = target_environment.create_read_write_txn()?;
        let missing_from_target = missing_trie_keys::<_, _, _, _, E>(
            &target_txn,
            target_store,
            vec![root.to_owned()],
            &Default::default(),
        )?;
        assert_eq!(missing_from_target, Vec::new());
        target_txn.commit()?;
    }

    // Make sure all of the target keys under the root hash are in the source
    {
        let source_txn: R::ReadTransaction = source_environment.create_read_txn()?;
        let target_txn: R::ReadTransaction = target_environment.create_read_txn()?;
        let target_keys = operations::keys::<_, _, _, _>(&target_txn, target_store, root)
            .collect::<Result<Vec<K>, S::Error>>()?;
        for key in target_keys {
            let maybe_value: ReadResult<V> =
                operations::read::<_, _, _, _, E>(&source_txn, source_store, root, &key)?;
            assert!(maybe_value.is_found())
        }
        source_txn.commit()?;
        target_txn.commit()?;
    }

    // Make sure all of the target keys under the root hash are in the source
    {
        let source_txn: R::ReadTransaction = source_environment.create_read_txn()?;
        let target_txn: R::ReadTransaction = target_environment.create_read_txn()?;
        let source_keys = operations::keys::<_, _, _, _>(&source_txn, source_store, root)
            .collect::<Result<Vec<K>, S::Error>>()?;
        for key in source_keys {
            let maybe_value: ReadResult<V> =
                operations::read::<_, _, _, _, E>(&target_txn, target_store, root, &key)?;
            assert!(maybe_value.is_found())
        }
        source_txn.commit()?;
        target_txn.commit()?;
    }

    Ok(())
}

#[test]
fn lmdb_copy_state() {
    let (root_hash, tries) = super::create_6_leaf_trie().unwrap();
    let source = LmdbTestContext::new(&tries).unwrap();
    let target = LmdbTestContext::new::<TestKey, TestValue>(&[]).unwrap();

    copy_state::<TestKey, TestValue, _, _, error::Error>(
        &source.environment,
        &source.store,
        &target.environment,
        &target.store,
        &root_hash,
    )
    .unwrap();
}
