use casper_hashing::Digest;
use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use crate::{
    shared::newtypes::CorrelationId,
    storage::{
        error,
        error::in_memory,
        trie_store::{
            operations::{
                self,
                tests::{InMemoryTestContext, RocksDbTestContext, TestKey, TestValue},
                ReadResult,
            },
            TrieStore,
        },
    },
};

fn copy_state<K, V, S, E>(
    correlation_id: CorrelationId,
    source_store: &S,
    target_store: &S,
    root: &Digest,
) -> Result<(), E>
where
    K: ToBytes + FromBytes + Eq + std::fmt::Debug + Copy + Clone + Ord,
    V: ToBytes + FromBytes + Eq + std::fmt::Debug + Copy,
    S: TrieStore<K, V>,
    E: From<S::Error> + From<bytesrepr::Error>,
{
    // Make sure no missing nodes in source
    {
        let missing_from_source = operations::missing_trie_keys::<_, _, _, E>(
            correlation_id,
            source_store,
            vec![root.to_owned()],
            &Default::default(),
        )?;
        assert_eq!(missing_from_source, Vec::new());
    }

    // Copy source to target
    {
        // Copy source to destination
        let mut queue = vec![root.to_owned()];
        while let Some(trie_key) = queue.pop() {
            let trie_bytes_to_insert = source_store
                .read_bytes(trie_key.as_ref())?
                .expect("should have trie");
            target_store.write_bytes(trie_key.as_ref(), trie_bytes_to_insert.as_ref())?;

            // Now that we've added in `trie_to_insert`, queue up its children
            let new_keys = operations::missing_trie_keys::<_, _, _, E>(
                correlation_id,
                target_store,
                vec![trie_key],
                &Default::default(),
            )?;

            queue.extend(new_keys);
        }
    }

    // After the copying process above there should be no missing entries in the target
    {
        let missing_from_target = operations::missing_trie_keys::<_, _, _, E>(
            correlation_id,
            target_store,
            vec![root.to_owned()],
            &Default::default(),
        )?;
        assert_eq!(missing_from_target, Vec::new());
    }

    // Make sure all of the target keys under the root hash are in the source
    {
        let target_keys = operations::keys(correlation_id, target_store, root)
            .collect::<Result<Vec<K>, S::Error>>()?;
        for key in target_keys {
            let maybe_value: ReadResult<V> =
                operations::read::<_, _, _, E>(correlation_id, source_store, root, &key)?;
            assert!(maybe_value.is_found())
        }
    }

    // Make sure all of the target keys under the root hash are in the source
    {
        let source_keys = operations::keys::<_, _, _>(correlation_id, source_store, root)
            .collect::<Result<Vec<K>, S::Error>>()?;
        for key in source_keys {
            let maybe_value: ReadResult<V> =
                operations::read::<_, _, _, E>(correlation_id, target_store, root, &key)?;
            assert!(maybe_value.is_found())
        }
    }

    Ok(())
}

#[test]
fn lmdb_copy_state() {
    let correlation_id = CorrelationId::new();
    let (root_hash, tries) = super::create_6_leaf_trie().unwrap();
    let source = RocksDbTestContext::new(&tries).unwrap();
    let target = RocksDbTestContext::new::<TestKey, TestValue>(&[]).unwrap();

    copy_state::<TestKey, TestValue, _, error::Error>(
        correlation_id,
        &source.store,
        &target.store,
        &root_hash,
    )
    .unwrap();
}

#[test]
fn in_memory_copy_state() {
    let correlation_id = CorrelationId::new();
    let (root_hash, tries) = super::create_6_leaf_trie().unwrap();
    let source = InMemoryTestContext::new(&tries).unwrap();
    let target = InMemoryTestContext::new::<TestKey, TestValue>(&[]).unwrap();

    copy_state::<TestKey, TestValue, _, in_memory::Error>(
        correlation_id,
        &source.store,
        &target.store,
        &root_hash,
    )
    .unwrap();
}
