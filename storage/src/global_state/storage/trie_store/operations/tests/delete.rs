use casper_types::Key;

use super::*;
use crate::global_state::storage::{
    transaction_source::Writable, trie_store::operations::DeleteResult,
};

fn checked_delete<T, S, E>(
    correlation_id: CorrelationId,
    txn: &mut T,
    store: &S,
    root: &Digest,
    key_to_delete: &Key,
) -> Result<DeleteResult, E>
where
    T: Readable<Handle = S::Handle> + Writable<Handle = S::Handle>,
    S: TrieStore,
    S::Error: From<T::Error>,
    E: From<S::Error> + From<bytesrepr::Error>,
{
    operations::delete::<T, S, E>(correlation_id, txn, store, root, key_to_delete)
}

mod partial_tries {
    use super::*;
    use crate::global_state::storage::trie_store::operations::DeleteResult;

    fn delete_from_partial_trie_had_expected_results<'a, R, S, E>(
        correlation_id: CorrelationId,
        environment: &'a R,
        store: &S,
        root: &Digest,
        key_to_delete: &Key,
        expected_root_after_delete: &Digest,
        expected_tries_after_delete: &[HashedTrie],
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        S: TrieStore,
        S::Error: From<R::Error>,
        E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
    {
        let mut txn = environment.create_read_write_txn()?;
        // The assert below only works with partial tries
        assert_eq!(store.get(&txn, expected_root_after_delete)?, None);
        let root_after_delete = match checked_delete::<_, _, E>(
            correlation_id,
            &mut txn,
            store,
            root,
            key_to_delete,
        )? {
            DeleteResult::Deleted(root_after_delete) => root_after_delete,
            DeleteResult::DoesNotExist => panic!("key did not exist"),
            DeleteResult::RootNotFound => panic!("root should be found"),
        };
        assert_eq!(root_after_delete, *expected_root_after_delete);
        for HashedTrie { hash, trie } in expected_tries_after_delete {
            assert_eq!(store.get(&txn, hash)?, Some(trie.clone()));
        }
        Ok(())
    }

    #[test]
    fn lmdb_delete_from_partial_trie_had_expected_results() {
        for i in 0..TEST_LEAVES_LENGTH {
            let correlation_id = CorrelationId::new();
            let (initial_root_hash, initial_tries) = TEST_TRIE_GENERATORS[i + 1]().unwrap();
            let (updated_root_hash, updated_tries) = TEST_TRIE_GENERATORS[i]().unwrap();
            let key_to_delete = &TEST_LEAVES[i];
            let context = LmdbTestContext::new(&initial_tries).unwrap();

            delete_from_partial_trie_had_expected_results::<_, _, error::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &initial_root_hash,
                key_to_delete.key().unwrap(),
                &updated_root_hash,
                updated_tries.as_slice(),
            )
            .unwrap();
        }
    }

    fn delete_non_existent_key_from_partial_trie_should_return_does_not_exist<'a, R, S, E>(
        correlation_id: CorrelationId,
        environment: &'a R,
        store: &S,
        root: &Digest,
        key_to_delete: &Key,
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        S: TrieStore,
        S::Error: From<R::Error>,
        E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
    {
        let mut txn = environment.create_read_write_txn()?;
        match checked_delete::<_, _, E>(correlation_id, &mut txn, store, root, key_to_delete)? {
            DeleteResult::Deleted(_) => panic!("should not delete"),
            DeleteResult::DoesNotExist => Ok(()),
            DeleteResult::RootNotFound => panic!("root should be found"),
        }
    }

    #[test]
    fn lmdb_delete_non_existent_key_from_partial_trie_should_return_does_not_exist() {
        for i in 0..TEST_LEAVES_LENGTH {
            let correlation_id = CorrelationId::new();
            let (initial_root_hash, initial_tries) = TEST_TRIE_GENERATORS[i]().unwrap();
            let key_to_delete = &TEST_LEAVES_ADJACENTS[i];
            let context = LmdbTestContext::new(&initial_tries).unwrap();

            delete_non_existent_key_from_partial_trie_should_return_does_not_exist::<
                _,
                _,
                error::Error,
            >(
                correlation_id,
                &context.environment,
                &context.store,
                &initial_root_hash,
                key_to_delete.key().unwrap(),
            )
            .unwrap();
        }
    }
}

mod full_tries {
    use std::ops::RangeInclusive;

    use once_cell::sync::Lazy;
    use proptest::{collection, proptest};

    use casper_types::{
        bytesrepr,
        gens::{colliding_key_arb, stored_value_arb},
        Key, StoredValue,
    };

    use crate::global_state::{
        shared::CorrelationId,
        storage::{
            error,
            transaction_source::TransactionSource,
            trie_store::{
                operations::{
                    delete, test_key, test_value,
                    tests::{LmdbTestContext, TEST_TRIE_GENERATORS},
                    write, DeleteResult, WriteResult,
                },
                TrieStore,
            },
        },
    };
    use casper_hashing::Digest;

    fn serially_insert_and_delete<'a, R, S, E>(
        correlation_id: CorrelationId,
        environment: &'a R,
        store: &S,
        root: &Digest,
        pairs: &[(Key, StoredValue)],
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        S: TrieStore,
        S::Error: From<R::Error>,
        E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
    {
        let mut txn = environment.create_read_write_txn()?;
        let mut roots = Vec::new();
        // Insert the key-value pairs, keeping track of the roots as we go
        for (key, value) in pairs {
            if let WriteResult::Written(new_root) = write::<_, _, E>(
                correlation_id,
                &mut txn,
                store,
                roots.last().unwrap_or(root),
                key,
                value,
            )? {
                roots.push(new_root);
            } else {
                panic!("Could not write pair")
            }
        }
        // Delete the key-value pairs, checking the resulting roots as we go
        let mut current_root = roots.pop().unwrap_or_else(|| root.to_owned());
        for (key, _value) in pairs.iter().rev() {
            if let DeleteResult::Deleted(new_root) =
                delete::<_, _, E>(correlation_id, &mut txn, store, &current_root, key)?
            {
                current_root = roots.pop().unwrap_or_else(|| root.to_owned());
                assert_eq!(new_root, current_root);
            } else {
                panic!("Could not delete")
            }
        }
        Ok(())
    }

    #[test]
    fn lmdb_serially_insert_and_delete() {
        let correlation_id = CorrelationId::new();
        let (empty_root_hash, empty_trie) = TEST_TRIE_GENERATORS[0]().unwrap();
        let context = LmdbTestContext::new(&empty_trie).unwrap();

        serially_insert_and_delete::<_, _, error::Error>(
            correlation_id,
            &context.environment,
            &context.store,
            &empty_root_hash,
            &[
                (test_key([1u8; 7]), test_value([1u8; 6])),
                (test_key([0u8; 7]), test_value([0u8; 6])),
                (test_key([0u8, 1, 1, 1, 1, 1, 1]), test_value([2u8; 6])),
                (test_key([2u8; 7]), test_value([2u8; 6])),
            ],
        )
        .unwrap();
    }

    const INTERLEAVED_INSERT_AND_DELETE_TEST_LEAVES_1: Lazy<[(Key, StoredValue); 3]> =
        Lazy::new(|| {
            [
                (test_key([1u8; 7]), test_value([1u8; 6])),
                (test_key([0u8; 7]), test_value([0u8; 6])),
                (test_key([0u8, 1, 1, 1, 1, 1, 1]), test_value([2u8; 6])),
            ]
        });

    const INTERLEAVED_DELETE_TEST_KEYS_1: Lazy<[Key; 1]> = Lazy::new(|| [test_key([1u8; 7])]);

    fn interleaved_insert_and_delete<'a, R, S, E>(
        correlation_id: CorrelationId,
        environment: &'a R,
        store: &S,
        root: &Digest,
        pairs_to_insert: &[(Key, StoredValue)],
        keys_to_delete: &[Key],
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        S: TrieStore,
        S::Error: From<R::Error>,
        E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
    {
        let mut txn = environment.create_read_write_txn()?;
        let mut expected_root = *root;
        // Insert the key-value pairs, keeping track of the roots as we go
        for (key, value) in pairs_to_insert.iter() {
            if let WriteResult::Written(new_root) =
                write::<_, _, E>(correlation_id, &mut txn, store, &expected_root, key, value)?
            {
                expected_root = new_root;
            } else {
                panic!("Could not write pair")
            }
        }
        for key in keys_to_delete.iter() {
            match delete::<_, _, E>(correlation_id, &mut txn, store, &expected_root, key)? {
                DeleteResult::Deleted(new_root) => {
                    expected_root = new_root;
                }
                DeleteResult::DoesNotExist => {}
                DeleteResult::RootNotFound => panic!("should find root"),
            }
        }

        let pairs_to_insert_less_deleted: Vec<(Key, StoredValue)> = pairs_to_insert
            .iter()
            .rev()
            .cloned()
            .filter(|(key, _value)| !keys_to_delete.contains(key))
            .collect();

        let mut actual_root = *root;
        for (key, value) in pairs_to_insert_less_deleted.iter() {
            if let WriteResult::Written(new_root) =
                write::<_, _, E>(correlation_id, &mut txn, store, &actual_root, key, value)?
            {
                actual_root = new_root;
            } else {
                panic!("Could not write pair")
            }
        }

        assert_eq!(expected_root, actual_root, "Expected did not match actual");

        Ok(())
    }

    #[test]
    fn lmdb_interleaved_insert_and_delete() {
        let correlation_id = CorrelationId::new();
        let (empty_root_hash, empty_trie) = TEST_TRIE_GENERATORS[0]().unwrap();
        let context = LmdbTestContext::new(&empty_trie).unwrap();

        interleaved_insert_and_delete::<_, _, error::Error>(
            correlation_id,
            &context.environment,
            &context.store,
            &empty_root_hash,
            &*INTERLEAVED_INSERT_AND_DELETE_TEST_LEAVES_1,
            &*INTERLEAVED_DELETE_TEST_KEYS_1,
        )
        .unwrap();
    }

    const DEFAULT_MIN_LENGTH: usize = 1;

    const DEFAULT_MAX_LENGTH: usize = 6;

    fn get_range() -> RangeInclusive<usize> {
        let start = option_env!("CL_TRIE_TEST_VECTOR_MIN_LENGTH")
            .and_then(|s| str::parse::<usize>(s).ok())
            .unwrap_or(DEFAULT_MIN_LENGTH);
        let end = option_env!("CL_TRIE_TEST_VECTOR_MAX_LENGTH")
            .and_then(|s| str::parse::<usize>(s).ok())
            .unwrap_or(DEFAULT_MAX_LENGTH);
        RangeInclusive::new(start, end)
    }

    proptest! {
        #[test]
        fn prop_lmdb_interleaved_insert_and_delete(
            pairs_to_insert in collection::vec((colliding_key_arb(), stored_value_arb()), get_range())
        ) {
            let correlation_id = CorrelationId::new();
            let (empty_root_hash, empty_trie) = TEST_TRIE_GENERATORS[0]().unwrap();
            let context = LmdbTestContext::new(&empty_trie).unwrap();

            let keys_to_delete = {
                let mut tmp = Vec::new();
                for i in (0..pairs_to_insert.len()).step_by(2) {
                    tmp.push(pairs_to_insert[i].0)
                }
                tmp
            };

            interleaved_insert_and_delete::<_, _, error::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &empty_root_hash,
                &pairs_to_insert,
                &keys_to_delete,
            )
            .unwrap();
        }
    }
}
