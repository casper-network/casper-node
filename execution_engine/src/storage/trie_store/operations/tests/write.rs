use super::*;

mod empty_tries {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn lmdb_non_colliding_writes_to_n_leaf_empty_trie_had_expected_results() {
        for num_leaves in 1..=TEST_LEAVES_LENGTH {
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = TEST_TRIE_GENERATORS[0]().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();
            let initial_states = vec![root_hash];

            writes_to_n_leaf_empty_trie_had_expected_results::<_, _, _, _, error::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &initial_states,
                &TEST_LEAVES_NON_COLLIDING[..num_leaves],
            )
            .unwrap();
        }
    }

    #[test]
    fn in_memory_non_colliding_writes_to_n_leaf_empty_trie_had_expected_results() {
        for num_leaves in 1..=TEST_LEAVES_LENGTH {
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = TEST_TRIE_GENERATORS[0]().unwrap();
            let context = InMemoryTestContext::new(&tries).unwrap();
            let initial_states = vec![root_hash];

            writes_to_n_leaf_empty_trie_had_expected_results::<_, _, _, _, in_memory::Error>(
                correlation_id,
                &context.environment,
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
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = TEST_TRIE_GENERATORS[0]().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();
            let initial_states = vec![root_hash];

            writes_to_n_leaf_empty_trie_had_expected_results::<_, _, _, _, error::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &initial_states,
                &TEST_LEAVES[..num_leaves],
            )
            .unwrap();
        }
    }

    #[test]
    fn in_memory_writes_to_n_leaf_empty_trie_had_expected_results() {
        for num_leaves in 1..=TEST_LEAVES_LENGTH {
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = TEST_TRIE_GENERATORS[0]().unwrap();
            let context = InMemoryTestContext::new(&tries).unwrap();
            let initial_states = vec![root_hash];

            writes_to_n_leaf_empty_trie_had_expected_results::<_, _, _, _, in_memory::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &initial_states,
                &TEST_LEAVES[..num_leaves],
            )
            .unwrap();
        }
    }

    #[test]
    fn in_memory_writes_to_n_leaf_empty_trie_had_expected_store_contents() {
        let expected_contents: HashMap<Digest, TestTrie> = {
            let mut ret = HashMap::new();
            for generator in &TEST_TRIE_GENERATORS {
                let (_, tries) = generator().unwrap();
                for HashedTestTrie { hash, trie } in tries {
                    ret.insert(hash, trie);
                }
            }
            ret
        };

        let actual_contents: HashMap<Digest, TestTrie> = {
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = TEST_TRIE_GENERATORS[0]().unwrap();
            let context = InMemoryTestContext::new(&tries).unwrap();

            write_leaves::<_, _, _, _, in_memory::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &root_hash,
                &TEST_LEAVES,
            )
            .unwrap();

            context.environment.dump(None).unwrap()
        };

        assert_eq!(expected_contents, actual_contents)
    }
}

mod partial_tries {
    use super::*;

    fn noop_writes_to_n_leaf_partial_trie_had_expected_results<'a, R, S, E>(
        correlation_id: CorrelationId,
        environment: &'a R,
        store: &S,
        states: &[Digest],
        num_leaves: usize,
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        S: TrieStore<TestKey, TestValue>,
        S::Error: From<R::Error>,
        E: From<R::Error> + From<S::Error> + From<bytesrepr::Error> + From<TrieHashingError>,
    {
        // Check that the expected set of leaves is in the trie
        check_leaves::<_, _, _, _, E>(
            correlation_id,
            environment,
            store,
            &states[0],
            &TEST_LEAVES[..num_leaves],
            &[],
        )?;

        // Rewrite that set of leaves
        let write_results = write_leaves::<_, _, _, _, E>(
            correlation_id,
            environment,
            store,
            &states[0],
            &TEST_LEAVES[..num_leaves],
        )?;

        assert!(write_results
            .iter()
            .all(|result| *result == WriteResult::AlreadyExists));

        // Check that the expected set of leaves is in the trie
        check_leaves::<_, _, _, _, E>(
            correlation_id,
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
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = generator().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();
            let states = vec![root_hash];

            noop_writes_to_n_leaf_partial_trie_had_expected_results::<_, _, error::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &states,
                num_leaves,
            )
            .unwrap()
        }
    }

    #[test]
    fn in_memory_noop_writes_to_n_leaf_partial_trie_had_expected_results() {
        for (num_leaves, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = generator().unwrap();
            let context = InMemoryTestContext::new(&tries).unwrap();
            let states = vec![root_hash];

            noop_writes_to_n_leaf_partial_trie_had_expected_results::<_, _, in_memory::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &states,
                num_leaves,
            )
            .unwrap();
        }
    }

    fn update_writes_to_n_leaf_partial_trie_had_expected_results<'a, R, S, E>(
        correlation_id: CorrelationId,
        environment: &'a R,
        store: &S,
        states: &[Digest],
        num_leaves: usize,
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        S: TrieStore<TestKey, TestValue>,
        S::Error: From<R::Error>,
        E: From<R::Error> + From<S::Error> + From<bytesrepr::Error> + From<TrieHashingError>,
    {
        let mut states = states.to_owned();

        // Check that the expected set of leaves is in the trie
        check_leaves::<_, _, _, _, E>(
            correlation_id,
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
                    correlation_id,
                    environment,
                    store,
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
                correlation_id,
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
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = generator().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();
            let initial_states = vec![root_hash];

            update_writes_to_n_leaf_partial_trie_had_expected_results::<_, _, error::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &initial_states,
                num_leaves,
            )
            .unwrap()
        }
    }

    #[test]
    fn in_memory_update_writes_to_n_leaf_partial_trie_had_expected_results() {
        for (num_leaves, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = generator().unwrap();
            let context = InMemoryTestContext::new(&tries).unwrap();
            let states = vec![root_hash];

            update_writes_to_n_leaf_partial_trie_had_expected_results::<_, _, in_memory::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &states,
                num_leaves,
            )
            .unwrap()
        }
    }
}

mod full_tries {
    use super::*;

    fn noop_writes_to_n_leaf_full_trie_had_expected_results<'a, R, S, E>(
        correlation_id: CorrelationId,
        environment: &'a R,
        store: &S,
        states: &[Digest],
        index: usize,
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        S: TrieStore<TestKey, TestValue>,
        S::Error: From<R::Error>,
        E: From<R::Error> + From<S::Error> + From<bytesrepr::Error> + From<TrieHashingError>,
    {
        // Check that the expected set of leaves is in the trie at every state reference
        for (num_leaves, state) in states[..index].iter().enumerate() {
            check_leaves::<_, _, _, _, E>(
                correlation_id,
                environment,
                store,
                state,
                &TEST_LEAVES[..num_leaves],
                &[],
            )?;
        }

        // Rewrite that set of leaves
        let write_results = write_leaves::<_, _, _, _, E>(
            correlation_id,
            environment,
            store,
            states.last().unwrap(),
            &TEST_LEAVES[..index],
        )?;

        assert!(write_results
            .iter()
            .all(|result| *result == WriteResult::AlreadyExists));

        // Check that the expected set of leaves is in the trie at every state reference
        for (num_leaves, state) in states[..index].iter().enumerate() {
            check_leaves::<_, _, _, _, E>(
                correlation_id,
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
        let correlation_id = CorrelationId::new();
        let context = LmdbTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for (index, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            noop_writes_to_n_leaf_full_trie_had_expected_results::<_, _, error::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &states,
                index,
            )
            .unwrap();
        }
    }

    #[test]
    fn in_memory_noop_writes_to_n_leaf_full_trie_had_expected_results() {
        let correlation_id = CorrelationId::new();
        let context = InMemoryTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for (index, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            noop_writes_to_n_leaf_full_trie_had_expected_results::<_, _, in_memory::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &states,
                index,
            )
            .unwrap();
        }
    }

    fn update_writes_to_n_leaf_full_trie_had_expected_results<'a, R, S, E>(
        correlation_id: CorrelationId,
        environment: &'a R,
        store: &S,
        states: &[Digest],
        num_leaves: usize,
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        S: TrieStore<TestKey, TestValue>,
        S::Error: From<R::Error>,
        E: From<R::Error> + From<S::Error> + From<bytesrepr::Error> + From<TrieHashingError>,
    {
        let mut states = states.to_vec();

        // Check that the expected set of leaves is in the trie at every state reference
        for (state_index, state) in states.iter().enumerate() {
            check_leaves::<_, _, _, _, E>(
                correlation_id,
                environment,
                store,
                state,
                &TEST_LEAVES[..state_index],
                &[],
            )?;
        }

        // Write set of leaves to the trie
        let hashes = write_leaves::<_, _, _, _, E>(
            correlation_id,
            environment,
            store,
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
            check_leaves::<_, _, _, _, E>(
                correlation_id,
                environment,
                store,
                state,
                &expected[state_index],
                &[],
            )?;
        }

        Ok(())
    }

    #[test]
    fn lmdb_update_writes_to_n_leaf_full_trie_had_expected_results() {
        let correlation_id = CorrelationId::new();
        let context = LmdbTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for (num_leaves, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            update_writes_to_n_leaf_full_trie_had_expected_results::<_, _, error::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &states,
                num_leaves,
            )
            .unwrap()
        }
    }

    #[test]
    fn in_memory_update_writes_to_n_leaf_full_trie_had_expected_results() {
        let correlation_id = CorrelationId::new();
        let context = InMemoryTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for (num_leaves, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            update_writes_to_n_leaf_full_trie_had_expected_results::<_, _, in_memory::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &states,
                num_leaves,
            )
            .unwrap()
        }
    }

    fn node_writes_to_5_leaf_full_trie_had_expected_results<'a, R, S, E>(
        correlation_id: CorrelationId,
        environment: &'a R,
        store: &S,
        states: &[Digest],
    ) -> Result<(), E>
    where
        R: TransactionSource<'a, Handle = S::Handle>,
        S: TrieStore<TestKey, TestValue>,
        S::Error: From<R::Error>,
        E: From<R::Error> + From<S::Error> + From<bytesrepr::Error> + From<TrieHashingError>,
    {
        let mut states = states.to_vec();
        let num_leaves = TEST_LEAVES_LENGTH;

        // Check that the expected set of leaves is in the trie at every state reference
        for (state_index, state) in states.iter().enumerate() {
            check_leaves::<_, _, _, _, E>(
                correlation_id,
                environment,
                store,
                state,
                &TEST_LEAVES[..state_index],
                &[],
            )?;
        }

        // Write set of leaves to the trie
        let hashes = write_leaves::<_, _, _, _, E>(
            correlation_id,
            environment,
            store,
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
            check_leaves::<_, _, _, _, E>(
                correlation_id,
                environment,
                store,
                state,
                &expected[state_index],
                &[],
            )?;
        }
        Ok(())
    }

    #[test]
    fn lmdb_node_writes_to_5_leaf_full_trie_had_expected_results() {
        let correlation_id = CorrelationId::new();
        let context = LmdbTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for generator in &TEST_TRIE_GENERATORS {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);
        }

        node_writes_to_5_leaf_full_trie_had_expected_results::<_, _, error::Error>(
            correlation_id,
            &context.environment,
            &context.store,
            &states,
        )
        .unwrap()
    }

    #[test]
    fn in_memory_node_writes_to_5_leaf_full_trie_had_expected_results() {
        let correlation_id = CorrelationId::new();
        let context = InMemoryTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for generator in &TEST_TRIE_GENERATORS {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);
        }

        node_writes_to_5_leaf_full_trie_had_expected_results::<_, _, in_memory::Error>(
            correlation_id,
            &context.environment,
            &context.store,
            &states,
        )
        .unwrap()
    }
}
