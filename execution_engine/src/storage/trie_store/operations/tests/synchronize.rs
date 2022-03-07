use casper_hashing::Digest;
use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use crate::{
    shared::newtypes::CorrelationId,
    storage::{
        error,
        error::in_memory,
        transaction_source::{Transaction, TransactionSource},
        trie_store::{
            operations::{
                self,
                tests::{InMemoryTestContext, LmdbTestContext, TestKey, TestValue},
                ReadResult,
            },
            TrieStore,
        },
    },
};

fn copy_state<'a, K, V, R, S, E>(
    correlation_id: CorrelationId,
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
        let missing_from_source = operations::missing_trie_keys::<_, _, _, _, E>(
            correlation_id,
            &txn,
            source_store,
            vec![root.to_owned()],
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
            target_store.put_raw(&mut target_txn, &trie_key, trie_bytes_to_insert.as_ref())?;

            // Now that we've added in `trie_to_insert`, queue up its children
            let new_keys = operations::missing_trie_keys::<_, _, _, _, E>(
                correlation_id,
                &target_txn,
                target_store,
                vec![trie_key],
            )?;

            queue.extend(new_keys);
        }
        source_txn.commit()?;
        target_txn.commit()?;
    }

    // After the copying process above there should be no missing entries in the target
    {
        let target_txn: R::ReadWriteTransaction = target_environment.create_read_write_txn()?;
        let missing_from_target = operations::missing_trie_keys::<_, _, _, _, E>(
            correlation_id,
            &target_txn,
            target_store,
            vec![root.to_owned()],
        )?;
        assert_eq!(missing_from_target, Vec::new());
        target_txn.commit()?;
    }

    // Make sure all of the target keys under the root hash are in the source
    {
        let source_txn: R::ReadTransaction = source_environment.create_read_txn()?;
        let target_txn: R::ReadTransaction = target_environment.create_read_txn()?;
        let target_keys =
            operations::keys::<_, _, _, _>(correlation_id, &target_txn, target_store, root)
                .collect::<Result<Vec<K>, S::Error>>()?;
        for key in target_keys {
            let maybe_value: ReadResult<V> = operations::read::<_, _, _, _, E>(
                correlation_id,
                &source_txn,
                source_store,
                root,
                &key,
            )?;
            assert!(maybe_value.is_found())
        }
        source_txn.commit()?;
        target_txn.commit()?;
    }

    // Make sure all of the target keys under the root hash are in the source
    {
        let source_txn: R::ReadTransaction = source_environment.create_read_txn()?;
        let target_txn: R::ReadTransaction = target_environment.create_read_txn()?;
        let source_keys =
            operations::keys::<_, _, _, _>(correlation_id, &source_txn, source_store, root)
                .collect::<Result<Vec<K>, S::Error>>()?;
        for key in source_keys {
            let maybe_value: ReadResult<V> = operations::read::<_, _, _, _, E>(
                correlation_id,
                &target_txn,
                target_store,
                root,
                &key,
            )?;
            assert!(maybe_value.is_found())
        }
        source_txn.commit()?;
        target_txn.commit()?;
    }

    Ok(())
}

#[test]
fn lmdb_copy_state() {
    let correlation_id = CorrelationId::new();
    let (root_hash, tries) = super::create_6_leaf_trie().unwrap();
    let source = LmdbTestContext::new(&tries).unwrap();
    let target = LmdbTestContext::new::<TestKey, TestValue>(&[]).unwrap();

    copy_state::<TestKey, TestValue, _, _, error::Error>(
        correlation_id,
        &source.environment,
        &source.store,
        &target.environment,
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

    copy_state::<TestKey, TestValue, _, _, in_memory::Error>(
        correlation_id,
        &source.environment,
        &source.store,
        &target.environment,
        &target.store,
        &root_hash,
    )
    .unwrap();
}
