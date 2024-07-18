use super::*;
use crate::global_state::trie_store::operations::TriePruneResult;

fn checked_prune<'a, K, V, R, WR, S, WS, E>(
    environment: &'a R,
    write_environment: &'a WR,
    store: &S,
    write_store: &WS,
    root: &Digest,
    key_to_prune: &K,
) -> Result<TriePruneResult, E>
where
    K: ToBytes + FromBytes + Clone + std::fmt::Debug + Eq,
    V: ToBytes + FromBytes + Clone + std::fmt::Debug,
    R: TransactionSource<'a, Handle = S::Handle>,
    WR: TransactionSource<'a, Handle = WS::Handle>,
    S: TrieStore<K, V>,
    WS: TrieStore<K, PanickingFromBytes<V>>,
    S::Error: From<R::Error>,
    WS::Error: From<WR::Error>,
    E: From<S::Error> + From<WS::Error> + From<R::Error> + From<WR::Error> + From<bytesrepr::Error>,
{
    let mut txn = write_environment.create_read_write_txn()?;
    let prune_result = operations::prune::<K, PanickingFromBytes<V>, _, WS, E>(
        &mut txn,
        write_store,
        root,
        key_to_prune,
    );
    txn.commit()?;
    let prune_result = prune_result?;
    let rtxn = environment.create_read_write_txn()?;
    if let TriePruneResult::Pruned(new_root) = prune_result {
        operations::check_integrity::<K, V, _, S, E>(&rtxn, store, vec![new_root])?;
    }
    rtxn.commit()?;
    Ok(prune_result)
}

mod partial_tries {
    use super::*;
    use crate::global_state::trie_store::operations::TriePruneResult;

    #[allow(clippy::too_many_arguments)]
    fn prune_from_partial_trie_had_expected_results<'a, K, V, R, WR, S, WS, E>(
        environment: &'a R,
        write_environment: &'a WR,
        store: &S,
        write_store: &WS,
        root: &Digest,
        key_to_prune: &K,
        expected_root_after_prune: &Digest,
        expected_tries_after_prune: &[HashedTrie<K, V>],
    ) -> Result<(), E>
    where
        K: ToBytes + FromBytes + Clone + Eq + std::fmt::Debug,
        V: ToBytes + FromBytes + Clone + Eq + std::fmt::Debug,
        R: TransactionSource<'a, Handle = S::Handle>,
        WR: TransactionSource<'a, Handle = WS::Handle>,
        S: TrieStore<K, V>,
        WS: TrieStore<K, PanickingFromBytes<V>>,
        S::Error: From<R::Error>,
        WS::Error: From<WR::Error>,
        E: From<R::Error>
            + From<S::Error>
            + From<WR::Error>
            + From<WS::Error>
            + From<bytesrepr::Error>,
    {
        let rtxn = environment.create_read_txn()?;
        // The assert below only works with partial tries
        assert_eq!(store.get(&rtxn, expected_root_after_prune)?, None);
        rtxn.commit()?;
        let root_after_prune = match checked_prune::<K, V, _, _, _, _, E>(
            environment,
            write_environment,
            store,
            write_store,
            root,
            key_to_prune,
        )? {
            TriePruneResult::Pruned(root_after_prune) => root_after_prune,
            TriePruneResult::MissingKey => panic!("key did not exist"),
            TriePruneResult::RootNotFound => panic!("root should be found"),
            TriePruneResult::Failure(err) => panic!("{:?}", err),
        };
        assert_eq!(root_after_prune, *expected_root_after_prune);
        let rtxn = environment.create_read_txn()?;
        for HashedTrie { hash, trie } in expected_tries_after_prune {
            assert_eq!(store.get(&rtxn, hash)?, Some(trie.clone()));
        }
        rtxn.commit()?;
        Ok(())
    }

    #[test]
    fn lmdb_prune_from_partial_trie_had_expected_results() {
        for i in 0..TEST_LEAVES_LENGTH {
            let (initial_root_hash, initial_tries) = TEST_TRIE_GENERATORS[i + 1]().unwrap();
            let (updated_root_hash, updated_tries) = TEST_TRIE_GENERATORS[i]().unwrap();
            let key_to_prune = &TEST_LEAVES[i];
            let context = LmdbTestContext::new(&initial_tries).unwrap();

            prune_from_partial_trie_had_expected_results::<
                TestKey,
                TestValue,
                _,
                _,
                _,
                _,
                error::Error,
            >(
                &context.environment,
                &context.environment,
                &context.store,
                &context.store,
                &initial_root_hash,
                key_to_prune.key().unwrap(),
                &updated_root_hash,
                updated_tries.as_slice(),
            )
            .unwrap();
        }
    }

    fn prune_non_existent_key_from_partial_trie_should_return_does_not_exist<
        'a,
        K,
        V,
        R,
        WR,
        S,
        WS,
        E,
    >(
        environment: &'a R,
        write_environment: &'a WR,
        store: &S,
        write_store: &WS,
        root: &Digest,
        key_to_prune: &K,
    ) -> Result<(), E>
    where
        K: ToBytes + FromBytes + Clone + Eq + std::fmt::Debug,
        V: ToBytes + FromBytes + Clone + Eq + std::fmt::Debug,
        R: TransactionSource<'a, Handle = S::Handle>,
        WR: TransactionSource<'a, Handle = WS::Handle>,
        S: TrieStore<K, V>,
        WS: TrieStore<K, PanickingFromBytes<V>>,
        S::Error: From<R::Error>,
        WS::Error: From<WR::Error>,
        E: From<R::Error>
            + From<S::Error>
            + From<bytesrepr::Error>
            + From<WR::Error>
            + From<WS::Error>,
    {
        match checked_prune::<K, _, _, _, _, _, E>(
            environment,
            write_environment,
            store,
            write_store,
            root,
            key_to_prune,
        )? {
            TriePruneResult::Pruned(_) => panic!("should not prune"),
            TriePruneResult::MissingKey => Ok(()),
            TriePruneResult::RootNotFound => panic!("root should be found"),
            TriePruneResult::Failure(err) => panic!("{:?}", err),
        }
    }

    #[test]
    fn lmdb_prune_non_existent_key_from_partial_trie_should_return_does_not_exist() {
        for i in 0..TEST_LEAVES_LENGTH {
            let (initial_root_hash, initial_tries) = TEST_TRIE_GENERATORS[i]().unwrap();
            let key_to_prune = &TEST_LEAVES_ADJACENTS[i];
            let context = LmdbTestContext::new(&initial_tries).unwrap();

            prune_non_existent_key_from_partial_trie_should_return_does_not_exist::<
                TestKey,
                TestValue,
                _,
                _,
                _,
                _,
                error::Error,
            >(
                &context.environment,
                &context.environment,
                &context.store,
                &context.store,
                &initial_root_hash,
                key_to_prune.key().unwrap(),
            )
            .unwrap();
        }
    }
}

mod full_tries {
    use super::*;
    use std::ops::RangeInclusive;

    use proptest::{collection, prelude::*};

    use casper_types::{
        bytesrepr::{self, FromBytes, ToBytes},
        gens::{colliding_key_arb, stored_value_arb},
        Digest, Key, StoredValue,
    };

    use crate::global_state::{
        error,
        transaction_source::TransactionSource,
        trie_store::{
            operations::{
                prune,
                tests::{LmdbTestContext, TestKey, TestValue, TEST_TRIE_GENERATORS},
                write, TriePruneResult, WriteResult,
            },
            TrieStore,
        },
    };

    fn serially_insert_and_prune<'a, K, V, R, S, E>(
        environment: &'a R,
        store: &S,
        root: &Digest,
        pairs: &[(K, V)],
    ) -> Result<(), E>
    where
        K: ToBytes + FromBytes + Clone + Eq + std::fmt::Debug,
        V: ToBytes + FromBytes + Clone + Eq + std::fmt::Debug,
        R: TransactionSource<'a, Handle = S::Handle>,
        S: TrieStore<K, PanickingFromBytes<V>>,
        S::Error: From<R::Error>,
        E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
    {
        let mut txn: R::ReadWriteTransaction = environment.create_read_write_txn()?;
        let mut roots = Vec::new();
        // Insert the key-value pairs, keeping track of the roots as we go
        for (key, value) in pairs {
            let new_value = PanickingFromBytes::new(value.clone());
            if let WriteResult::Written(new_root) = write::<K, PanickingFromBytes<V>, _, _, E>(
                &mut txn,
                store,
                roots.last().unwrap_or(root),
                key,
                &new_value,
            )? {
                roots.push(new_root);
            } else {
                panic!("Could not write pair")
            }
        }
        // Delete the key-value pairs, checking the resulting roots as we go
        let mut current_root = roots.pop().unwrap_or_else(|| root.to_owned());
        for (key, _value) in pairs.iter().rev() {
            let prune_result =
                prune::<K, PanickingFromBytes<V>, _, _, E>(&mut txn, store, &current_root, key);
            if let TriePruneResult::Pruned(new_root) = prune_result? {
                current_root = roots.pop().unwrap_or_else(|| root.to_owned());
                assert_eq!(new_root, current_root);
            } else {
                panic!("Could not prune")
            }
        }
        Ok(())
    }

    #[test]
    fn lmdb_serially_insert_and_prune() {
        let (empty_root_hash, empty_trie) = TEST_TRIE_GENERATORS[0]().unwrap();
        let context = LmdbTestContext::new(&empty_trie).unwrap();

        serially_insert_and_prune::<TestKey, TestValue, _, _, error::Error>(
            &context.environment,
            &context.store,
            &empty_root_hash,
            &[
                (TestKey([1u8; 7]), TestValue([1u8; 6])),
                (TestKey([0u8; 7]), TestValue([0u8; 6])),
                (TestKey([0u8, 1, 1, 1, 1, 1, 1]), TestValue([2u8; 6])),
                (TestKey([2u8; 7]), TestValue([2u8; 6])),
            ],
        )
        .unwrap();
    }

    const INTERLEAVED_INSERT_AND_PRUNE_TEST_LEAVES_1: [(TestKey, TestValue); 3] = [
        (TestKey([1u8; 7]), TestValue([1u8; 6])),
        (TestKey([0u8; 7]), TestValue([0u8; 6])),
        (TestKey([0u8, 1, 1, 1, 1, 1, 1]), TestValue([2u8; 6])),
    ];

    const INTERLEAVED_PRUNE_TEST_KEYS_1: [TestKey; 1] = [TestKey([1u8; 7])];

    fn interleaved_insert_and_prune<'a, K, V, R, S, E>(
        environment: &'a R,
        store: &S,
        root: &Digest,
        pairs_to_insert: &[(K, V)],
        keys_to_prune: &[K],
    ) -> Result<(), E>
    where
        K: ToBytes + FromBytes + Clone + Eq + std::fmt::Debug,
        V: ToBytes + FromBytes + Clone + Eq + std::fmt::Debug,
        R: TransactionSource<'a, Handle = S::Handle>,
        S: TrieStore<K, PanickingFromBytes<V>>,
        S::Error: From<R::Error>,
        E: From<R::Error> + From<S::Error> + From<bytesrepr::Error>,
    {
        let mut txn: R::ReadWriteTransaction = environment.create_read_write_txn()?;
        let mut expected_root = *root;
        // Insert the key-value pairs, keeping track of the roots as we go
        for (key, value) in pairs_to_insert.iter() {
            let new_value = PanickingFromBytes::new(value.clone());
            if let WriteResult::Written(new_root) = write::<K, PanickingFromBytes<V>, _, _, E>(
                &mut txn,
                store,
                &expected_root,
                key,
                &new_value,
            )? {
                expected_root = new_root;
            } else {
                panic!("Could not write pair")
            }
        }
        for key in keys_to_prune.iter() {
            let prune_result =
                prune::<K, PanickingFromBytes<V>, _, _, E>(&mut txn, store, &expected_root, key);
            match prune_result? {
                TriePruneResult::Pruned(new_root) => {
                    expected_root = new_root;
                }
                TriePruneResult::MissingKey => {}
                TriePruneResult::RootNotFound => panic!("should find root"),
                TriePruneResult::Failure(err) => panic!("{:?}", err),
            }
        }

        let pairs_to_insert_less_pruned: Vec<(K, V)> = pairs_to_insert
            .iter()
            .rev()
            .filter(|&(key, _value)| !keys_to_prune.contains(key))
            .cloned()
            .collect();

        let mut actual_root = *root;
        for (key, value) in pairs_to_insert_less_pruned.iter() {
            let new_value = PanickingFromBytes::new(value.clone());
            if let WriteResult::Written(new_root) = write::<K, PanickingFromBytes<V>, _, _, E>(
                &mut txn,
                store,
                &actual_root,
                key,
                &new_value,
            )? {
                actual_root = new_root;
            } else {
                panic!("Could not write pair")
            }
        }

        assert_eq!(expected_root, actual_root, "Expected did not match actual");

        Ok(())
    }

    #[test]
    fn lmdb_interleaved_insert_and_prune() {
        let (empty_root_hash, empty_trie) = TEST_TRIE_GENERATORS[0]().unwrap();
        let context = LmdbTestContext::new(&empty_trie).unwrap();

        interleaved_insert_and_prune::<TestKey, TestValue, _, _, error::Error>(
            &context.environment,
            &context.store,
            &empty_root_hash,
            &INTERLEAVED_INSERT_AND_PRUNE_TEST_LEAVES_1,
            &INTERLEAVED_PRUNE_TEST_KEYS_1,
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
        fn prop_lmdb_interleaved_insert_and_prune(
            pairs_to_insert in collection::vec((colliding_key_arb(), stored_value_arb()), get_range())
        ) {
            let (empty_root_hash, empty_trie) = TEST_TRIE_GENERATORS[0]().unwrap();
            let context = LmdbTestContext::new(&empty_trie).unwrap();

            let keys_to_prune = {
                let mut tmp = Vec::new();
                for i in (0..pairs_to_insert.len()).step_by(2) {
                    tmp.push(pairs_to_insert[i].0)
                }
                tmp
            };

            interleaved_insert_and_prune::<Key, StoredValue, _, _, error::Error>(
                &context.environment,
                &context.store,
                &empty_root_hash,
                &pairs_to_insert,
                &keys_to_prune,
            )
            .unwrap();
        }
    }
}
