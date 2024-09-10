use super::*;

mod empty_tries {
    use super::*;

    #[test]
    fn lmdb_non_colliding_writes_to_n_leaf_empty_trie_had_expected_results() {
        for num_leaves in 1..=TEST_LEAVES_LENGTH {
            let (root_hash, tries) = TEST_TRIE_GENERATORS[0]().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();
            let initial_states = vec![root_hash];

            writes_to_n_leaf_empty_trie_had_expected_results::<_, _, _, _, _, _, error::Error>(
                &context.environment,
                &context.environment,
                &context.store,
                &context.store,
                &initial_states,
                &TEST_LEAVES_NON_COLLIDING[..num_leaves],
            )
            .unwrap();
        }
    }

    #[test]
    fn lmdb_writes_to_n_leaf_empty_trie_had_expected_results() {
        for num_leaves in 1..=TEST_LEAVES_LENGTH {
            let (root_hash, tries) = TEST_TRIE_GENERATORS[0]().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();
            let initial_states = vec![root_hash];

            writes_to_n_leaf_empty_trie_had_expected_results::<_, _, _, _, _, _, error::Error>(
                &context.environment,
                &context.environment,
                &context.store,
                &context.store,
                &initial_states,
                &TEST_LEAVES[..num_leaves],
            )
            .unwrap();
        }
    }
}

mod partial_tries {
    use super::*;

    fn noop_writes_to_n_leaf_partial_trie_had_expected_results<'a, R, WR, S, WS, E>(
        environment: &'a R,
        write_environment: &'a WR,
        store: &S,
        writable_store: &WS,
        states: &[Digest],
        num_leaves: usize,
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        WR: TransactionSource<'a, Handle = WS::Handle>,
        S: TrieStore<TestKey, TestValue>,
        WS: TrieStore<TestKey, PanickingFromBytes<TestValue>>,
        S::Error: From<R::Error>,
        WS::Error: From<WR::Error>,
        E: From<R::Error>
            + From<S::Error>
            + From<bytesrepr::Error>
            + From<WR::Error>
            + From<WS::Error>,
    {
        // Check that the expected set of leaves is in the trie
        check_leaves::<_, _, _, _, E>(
            environment,
            store,
            &states[0],
            &TEST_LEAVES[..num_leaves],
            &[],
        )?;

        // Rewrite that set of leaves
        let write_results = write_leaves::<TestKey, _, WR, WS, E>(
            write_environment,
            writable_store,
            &states[0],
            &TEST_LEAVES[..num_leaves],
        )?;

        assert!(write_results
            .iter()
            .all(|result| *result == WriteResult::AlreadyExists));

        // Check that the expected set of leaves is in the trie
        check_leaves::<_, _, _, _, E>(
            environment,
            store,
            &states[0],
            &TEST_LEAVES[..num_leaves],
            &[],
        )
    }

    #[test]
    fn lmdb_noop_writes_to_n_leaf_partial_trie_had_expected_results() {
        for (num_leaves, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();
            let states = vec![root_hash];

            noop_writes_to_n_leaf_partial_trie_had_expected_results::<_, _, _, _, error::Error>(
                &context.environment,
                &context.environment,
                &context.store,
                &context.store,
                &states,
                num_leaves,
            )
            .unwrap();
        }
    }

    fn update_writes_to_n_leaf_partial_trie_had_expected_results<'a, R, WR, S, WS, E>(
        environment: &'a R,
        write_environment: &'a WR,
        store: &S,
        writable_store: &WS,
        states: &[Digest],
        num_leaves: usize,
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        WR: TransactionSource<'a, Handle = WS::Handle>,
        S: TrieStore<TestKey, TestValue>,
        WS: TrieStore<TestKey, PanickingFromBytes<TestValue>>,
        S::Error: From<R::Error>,
        WS::Error: From<WR::Error>,
        E: From<R::Error>
            + From<S::Error>
            + From<bytesrepr::Error>
            + From<WR::Error>
            + From<WS::Error>,
    {
        let mut states = states.to_owned();

        // Check that the expected set of leaves is in the trie
        check_leaves::<_, _, _, _, E>(
            environment,
            store,
            &states[0],
            &TEST_LEAVES[..num_leaves],
            &[],
        )?;

        // Update and check leaves
        for (n, leaf) in TEST_LEAVES_UPDATED[..num_leaves].iter().enumerate() {
            let expected_leaves: Vec<TestTrie> = {
                let n = n + 1;
                TEST_LEAVES_UPDATED[..n]
                    .iter()
                    .chain(&TEST_LEAVES[n..num_leaves])
                    .map(ToOwned::to_owned)
                    .collect()
            };

            let root_hash = {
                let current_root = states.last().unwrap();
                let results = write_leaves::<_, _, _, _, E>(
                    write_environment,
                    writable_store,
                    current_root,
                    &[leaf.to_owned()],
                )?;
                assert_eq!(1, results.len());
                match results[0] {
                    WriteResult::Written(root_hash) => root_hash,
                    _ => panic!("value not written"),
                }
            };

            states.push(root_hash);

            // Check that the expected set of leaves is in the trie
            check_leaves::<_, _, _, _, E>(
                environment,
                store,
                states.last().unwrap(),
                &expected_leaves,
                &[],
            )?;
        }

        Ok(())
    }

    #[test]
    fn lmdb_update_writes_to_n_leaf_partial_trie_had_expected_results() {
        for (num_leaves, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();
            let initial_states = vec![root_hash];

            update_writes_to_n_leaf_partial_trie_had_expected_results::<_, _, _, _, error::Error>(
                &context.environment,
                &context.environment,
                &context.store,
                &context.store,
                &initial_states,
                num_leaves,
            )
            .unwrap()
        }
    }
}

mod full_tries {
    use super::*;

    fn noop_writes_to_n_leaf_full_trie_had_expected_results<'a, R, WR, S, WS, E>(
        environment: &'a R,
        write_environment: &'a WR,
        store: &S,
        write_store: &WS,
        states: &[Digest],
        index: usize,
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        WR: TransactionSource<'a, Handle = WS::Handle>,
        S: TrieStore<TestKey, TestValue>,
        WS: TrieStore<TestKey, PanickingFromBytes<TestValue>>,
        S::Error: From<R::Error>,
        WS::Error: From<WR::Error>,
        E: From<R::Error>
            + From<S::Error>
            + From<bytesrepr::Error>
            + From<WR::Error>
            + From<WS::Error>,
    {
        // Check that the expected set of leaves is in the trie at every state reference
        for (num_leaves, state) in states[..index].iter().enumerate() {
            check_leaves::<_, _, _, _, E>(
                environment,
                store,
                state,
                &TEST_LEAVES[..num_leaves],
                &[],
            )?;
        }

        // Rewrite that set of leaves
        let write_results = write_leaves::<_, _, _, _, E>(
            write_environment,
            write_store,
            states.last().unwrap(),
            &TEST_LEAVES[..index],
        )?;

        assert!(write_results
            .iter()
            .all(|result| *result == WriteResult::AlreadyExists));

        // Check that the expected set of leaves is in the trie at every state reference
        for (num_leaves, state) in states[..index].iter().enumerate() {
            check_leaves::<_, _, _, _, E>(
                environment,
                store,
                state,
                &TEST_LEAVES[..num_leaves],
                &[],
            )?
        }

        Ok(())
    }

    #[test]
    fn lmdb_noop_writes_to_n_leaf_full_trie_had_expected_results() {
        let context = LmdbTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for (index, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            noop_writes_to_n_leaf_full_trie_had_expected_results::<_, _, _, _, error::Error>(
                &context.environment,
                &context.environment,
                &context.store,
                &context.store,
                &states,
                index,
            )
            .unwrap();
        }
    }

    fn update_writes_to_n_leaf_full_trie_had_expected_results<'a, R, WR, S, WS, E>(
        environment: &'a R,
        write_environment: &'a WR,
        store: &S,
        write_store: &WS,
        states: &[Digest],
        num_leaves: usize,
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        WR: TransactionSource<'a, Handle = WS::Handle>,
        S: TrieStore<TestKey, TestValue>,
        WS: TrieStore<TestKey, PanickingFromBytes<TestValue>>,
        S::Error: From<R::Error>,
        WS::Error: From<WR::Error>,
        E: From<R::Error>
            + From<S::Error>
            + From<bytesrepr::Error>
            + From<WR::Error>
            + From<WS::Error>,
    {
        let mut states = states.to_vec();

        // Check that the expected set of leaves is in the trie at every state reference
        for (state_index, state) in states.iter().enumerate() {
            check_leaves::<_, _, _, _, E>(
                environment,
                store,
                state,
                &TEST_LEAVES[..state_index],
                &[],
            )?;
        }

        // Write set of leaves to the trie
        let hashes = write_leaves::<_, _, _, _, E>(
            write_environment,
            write_store,
            states.last().unwrap(),
            &TEST_LEAVES_UPDATED[..num_leaves],
        )?
        .iter()
        .map(|result| match result {
            WriteResult::Written(root_hash) => *root_hash,
            _ => panic!("write_leaves resulted in non-write"),
        })
        .collect::<Vec<Digest>>();

        states.extend(hashes);

        let expected: Vec<Vec<TestTrie>> = {
            let mut ret = vec![vec![]];
            if num_leaves > 0 {
                for i in 1..=num_leaves {
                    ret.push(TEST_LEAVES[..i].to_vec())
                }
                for i in 1..=num_leaves {
                    ret.push(
                        TEST_LEAVES[i..num_leaves]
                            .iter()
                            .chain(&TEST_LEAVES_UPDATED[..i])
                            .map(ToOwned::to_owned)
                            .collect::<Vec<TestTrie>>(),
                    )
                }
            }
            ret
        };

        assert_eq!(states.len(), expected.len());

        // Check that the expected set of leaves is in the trie at every state reference
        for (state_index, state) in states.iter().enumerate() {
            check_leaves::<_, _, _, _, E>(environment, store, state, &expected[state_index], &[])?;
        }

        Ok(())
    }

    #[test]
    fn lmdb_update_writes_to_n_leaf_full_trie_had_expected_results() {
        let context = LmdbTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for (num_leaves, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            update_writes_to_n_leaf_full_trie_had_expected_results::<_, _, _, _, error::Error>(
                &context.environment,
                &context.environment,
                &context.store,
                &context.store,
                &states,
                num_leaves,
            )
            .unwrap()
        }
    }

    fn node_writes_to_5_leaf_full_trie_had_expected_results<'a, R, WR, S, WS, E>(
        environment: &'a R,
        write_environment: &'a WR,
        store: &S,
        write_store: &WS,
        states: &[Digest],
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        WR: TransactionSource<'a, Handle = WS::Handle>,
        S: TrieStore<TestKey, TestValue>,
        WS: TrieStore<TestKey, PanickingFromBytes<TestValue>>,
        S::Error: From<R::Error>,
        WS::Error: From<WR::Error>,
        E: From<R::Error>
            + From<S::Error>
            + From<bytesrepr::Error>
            + From<WR::Error>
            + From<WS::Error>,
    {
        let mut states = states.to_vec();
        let num_leaves = TEST_LEAVES_LENGTH;

        // Check that the expected set of leaves is in the trie at every state reference
        for (state_index, state) in states.iter().enumerate() {
            check_leaves::<_, _, _, _, E>(
                environment,
                store,
                state,
                &TEST_LEAVES[..state_index],
                &[],
            )?;
        }

        // Write set of leaves to the trie
        let hashes = write_leaves::<_, _, _, _, E>(
            write_environment,
            write_store,
            states.last().unwrap(),
            &TEST_LEAVES_ADJACENTS,
        )?
        .iter()
        .map(|result| match result {
            WriteResult::Written(root_hash) => *root_hash,
            _ => panic!("write_leaves resulted in non-write"),
        })
        .collect::<Vec<Digest>>();

        states.extend(hashes);

        let expected: Vec<Vec<TestTrie>> = {
            let mut ret = vec![vec![]];
            if num_leaves > 0 {
                for i in 1..=num_leaves {
                    ret.push(TEST_LEAVES[..i].to_vec())
                }
                for i in 1..=num_leaves {
                    ret.push(
                        TEST_LEAVES
                            .iter()
                            .chain(&TEST_LEAVES_ADJACENTS[..i])
                            .map(ToOwned::to_owned)
                            .collect::<Vec<TestTrie>>(),
                    )
                }
            }
            ret
        };

        assert_eq!(states.len(), expected.len());

        // Check that the expected set of leaves is in the trie at every state reference
        for (state_index, state) in states.iter().enumerate() {
            check_leaves::<_, _, _, _, E>(environment, store, state, &expected[state_index], &[])?;
        }
        Ok(())
    }

    #[test]
    fn lmdb_node_writes_to_5_leaf_full_trie_had_expected_results() {
        let context = LmdbTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for generator in &TEST_TRIE_GENERATORS {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);
        }

        node_writes_to_5_leaf_full_trie_had_expected_results::<_, _, _, _, error::Error>(
            &context.environment,
            &context.environment,
            &context.store,
            &context.store,
            &states,
        )
        .unwrap()
    }
}

mod variable_sized_keys {
    use super::*;

    fn assert_write_result(result: WriteResult) -> Option<Digest> {
        match result {
            WriteResult::Written(root_hash) => Some(root_hash),
            WriteResult::AlreadyExists => None,
            WriteResult::RootNotFound => panic!("Root not found while attempting write"),
        }
    }

    #[test]
    fn write_variable_len_keys() {
        let (root_hash, tries) = create_empty_trie::<MultiVariantTestKey, u32>().unwrap();

        let context = LmdbTestContext::new(&tries).unwrap();
        let mut txn = context.environment.create_read_write_txn().unwrap();

        let test_key_1 =
            MultiVariantTestKey::VariableSizedKey(VariableAddr::LegacyAddr(*b"caab6ff"));
        let root_hash = assert_write_result(
            write::<MultiVariantTestKey, _, _, _, error::Error>(
                &mut txn,
                &context.store,
                &root_hash,
                &test_key_1,
                &1u32,
            )
            .unwrap(),
        )
        .expect("Expected new root hash after write");

        let test_key_2 =
            MultiVariantTestKey::VariableSizedKey(VariableAddr::LegacyAddr(*b"caabb74"));
        let root_hash = assert_write_result(
            write::<MultiVariantTestKey, _, _, _, error::Error>(
                &mut txn,
                &context.store,
                &root_hash,
                &test_key_2,
                &2u32,
            )
            .unwrap(),
        )
        .expect("Expected new root hash after write");

        let test_key_3 = MultiVariantTestKey::VariableSizedKey(VariableAddr::Empty);
        let _ = assert_write_result(
            write::<MultiVariantTestKey, _, _, _, error::Error>(
                &mut txn,
                &context.store,
                &root_hash,
                &test_key_3,
                &3u32,
            )
            .unwrap(),
        )
        .expect("Expected new root hash after write");
    }
}

mod batch_write_with_random_keys {
    use crate::global_state::trie_store::cache::TrieCache;

    use super::*;

    use casper_types::{testing::TestRng, Key};
    use rand::Rng;

    #[test]
    fn compare_random_keys_seq_write_with_batch_cache_write() {
        let mut rng = TestRng::new();

        for _ in 0..100 {
            let (mut seq_write_root_hash, tries) = create_empty_trie::<Key, u32>().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();
            let mut txn = context.environment.create_read_write_txn().unwrap();

            // Create some random keys and values.
            let data: Vec<(Key, u32)> = (0u32..4000).map(|val| (rng.gen(), val)).collect();

            // Write all the keys sequentially to the store
            for (key, value) in data.iter() {
                let write_result = write::<Key, u32, _, _, error::Error>(
                    &mut txn,
                    &context.store,
                    &seq_write_root_hash,
                    key,
                    value,
                )
                .unwrap();
                match write_result {
                    WriteResult::Written(hash) => {
                        seq_write_root_hash = hash; // Update the state root hash; we'll use it to
                                                    // compare with the cache root hash.
                    }
                    WriteResult::AlreadyExists => (),
                    WriteResult::RootNotFound => panic!("write_leaves given an invalid root"),
                };
            }

            // Create an empty store that backs up the cache.
            let (cache_root_hash, tries) = create_empty_trie::<Key, u32>().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();
            let mut txn = context.environment.create_read_write_txn().unwrap();

            let mut trie_cache = TrieCache::<Key, u32, _>::new::<_, error::Error>(
                &txn,
                &context.store,
                &cache_root_hash,
            )
            .unwrap();
            for (key, value) in data.iter() {
                trie_cache
                    .insert::<_, error::Error>(*key, *value, &txn)
                    .unwrap();
            }

            let cache_root_hash = trie_cache.store_cache::<_, error::Error>(&mut txn).unwrap();

            if seq_write_root_hash != cache_root_hash {
                println!("Root Hash is: {:?}", seq_write_root_hash);
                println!("Cache root Hash is: {:?}", cache_root_hash);
                println!("Faulty keys: ");

                for (key, _) in data.iter() {
                    println!("{}", key.to_formatted_string());
                }
                panic!("ROOT hash mismatch");
            }
        }
    }

    #[test]
    fn compare_random_keys_write_with_cache_and_readback() {
        let mut rng = TestRng::new();

        // create a store
        let (mut root_hash, tries) = create_empty_trie::<Key, u32>().unwrap();
        let context = LmdbTestContext::new(&tries).unwrap();
        let mut txn = context.environment.create_read_write_txn().unwrap();

        // Create initial keys and values.
        let initial_keys: Vec<(Key, u32)> = (0u32..1000).map(|val| (rng.gen(), val)).collect();

        // Store these keys and values using sequential write;
        for (key, value) in initial_keys.iter() {
            let write_result = write::<Key, u32, _, _, error::Error>(
                &mut txn,
                &context.store,
                &root_hash,
                key,
                value,
            )
            .unwrap();
            match write_result {
                WriteResult::Written(hash) => {
                    root_hash = hash;
                }
                WriteResult::AlreadyExists => (),
                WriteResult::RootNotFound => panic!("write_leaves given an invalid root"),
            };
        }

        // Create some test data.
        let data: Vec<(Key, u32)> = (0u32..1000).map(|val| (rng.gen(), val)).collect();

        // Create a cache backed up by the store that has the initial data.
        let mut trie_cache =
            TrieCache::<Key, u32, _>::new::<_, error::Error>(&txn, &context.store, &root_hash)
                .unwrap();

        // Insert the test data into the cache.
        for (key, value) in data.iter() {
            trie_cache
                .insert::<_, error::Error>(*key, *value, &txn)
                .unwrap();
        }

        // Get the generated root hash
        let cache_root_hash = trie_cache.calculate_root_hash();

        // now write the same keys to the store one by one and check if we get the same root hash.
        let mut seq_write_root_hash = root_hash;
        for (key, value) in data.iter() {
            let write_result = write::<Key, u32, _, _, error::Error>(
                &mut txn,
                &context.store,
                &seq_write_root_hash,
                key,
                value,
            )
            .unwrap();
            match write_result {
                WriteResult::Written(hash) => {
                    seq_write_root_hash = hash;
                }
                WriteResult::AlreadyExists => (),
                WriteResult::RootNotFound => panic!("write_leaves given an invalid root"),
            };
        }

        assert_eq!(cache_root_hash, seq_write_root_hash);
    }
}
