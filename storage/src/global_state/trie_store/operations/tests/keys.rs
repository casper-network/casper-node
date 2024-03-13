mod partial_tries {
    use crate::global_state::{
        transaction_source::{Transaction, TransactionSource},
        trie::Trie,
        trie_store::operations::{
            self,
            tests::{LmdbTestContext, TestKey, TestValue, TEST_LEAVES, TEST_TRIE_GENERATORS},
        },
    };

    #[test]
    fn lmdb_keys_from_n_leaf_partial_trie_had_expected_results() {
        for (num_leaves, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();
            let test_leaves = TEST_LEAVES;
            let (used, _) = test_leaves.split_at(num_leaves);

            let expected = {
                let mut tmp = used
                    .iter()
                    .filter_map(Trie::key)
                    .cloned()
                    .collect::<Vec<TestKey>>();
                tmp.sort();
                tmp
            };
            let actual = {
                let txn = context.environment.create_read_txn().unwrap();
                let mut tmp =
                    operations::keys::<TestKey, TestValue, _, _>(&txn, &context.store, &root_hash)
                        .filter_map(Result::ok)
                        .collect::<Vec<TestKey>>();
                txn.commit().unwrap();
                tmp.sort();
                tmp
            };
            assert_eq!(actual, expected);
        }
    }
}

mod full_tries {
    use casper_types::Digest;

    use crate::global_state::{
        transaction_source::{Transaction, TransactionSource},
        trie::Trie,
        trie_store::operations::{
            self,
            tests::{
                LmdbTestContext, TestKey, TestValue, EMPTY_HASHED_TEST_TRIES, TEST_LEAVES,
                TEST_TRIE_GENERATORS,
            },
        },
    };

    #[test]
    fn lmdb_keys_from_n_leaf_full_trie_had_expected_results() {
        let context = LmdbTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for (state_index, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            for (num_leaves, state) in states[..state_index].iter().enumerate() {
                let test_leaves = TEST_LEAVES;
                let (used, _unused) = test_leaves.split_at(num_leaves);

                let expected = {
                    let mut tmp = used
                        .iter()
                        .filter_map(Trie::key)
                        .cloned()
                        .collect::<Vec<TestKey>>();
                    tmp.sort();
                    tmp
                };
                let actual = {
                    let txn = context.environment.create_read_txn().unwrap();
                    let mut tmp =
                        operations::keys::<TestKey, TestValue, _, _>(&txn, &context.store, state)
                            .filter_map(Result::ok)
                            .collect::<Vec<TestKey>>();
                    txn.commit().unwrap();
                    tmp.sort();
                    tmp
                };
                assert_eq!(actual, expected);
            }
        }
    }
}

#[cfg(debug_assertions)]
mod keys_iterator {
    use casper_types::{bytesrepr, global_state::Pointer, Digest};

    use crate::global_state::{
        transaction_source::TransactionSource,
        trie::Trie,
        trie_store::operations::{
            self,
            tests::{
                hash_test_tries, HashedTestTrie, HashedTrie, LmdbTestContext, TestKey, TestValue,
                TEST_LEAVES,
            },
        },
    };

    fn create_invalid_extension_trie() -> Result<(Digest, Vec<HashedTestTrie>), bytesrepr::Error> {
        let leaves = hash_test_tries(&TEST_LEAVES[2..3])?;
        let ext_1 = HashedTrie::new(Trie::extension(
            vec![0u8, 0],
            Pointer::NodePointer(leaves[0].hash),
        ))?;

        let root = HashedTrie::new(Trie::node(&[(0, Pointer::NodePointer(ext_1.hash))]))?;
        let root_hash = root.hash;

        let tries = vec![root, ext_1, leaves[0].clone()];

        Ok((root_hash, tries))
    }

    fn create_invalid_path_trie() -> Result<(Digest, Vec<HashedTestTrie>), bytesrepr::Error> {
        let leaves = hash_test_tries(&TEST_LEAVES[..1])?;

        let root = HashedTrie::new(Trie::node(&[(1, Pointer::NodePointer(leaves[0].hash))]))?;
        let root_hash = root.hash;

        let tries = vec![root, leaves[0].clone()];

        Ok((root_hash, tries))
    }

    fn create_invalid_hash_trie() -> Result<(Digest, Vec<HashedTestTrie>), bytesrepr::Error> {
        let leaves = hash_test_tries(&TEST_LEAVES[..2])?;

        let root = HashedTrie::new(Trie::node(&[(0, Pointer::NodePointer(leaves[1].hash))]))?;
        let root_hash = root.hash;

        let tries = vec![root, leaves[0].clone()];

        Ok((root_hash, tries))
    }

    macro_rules! return_on_err {
        ($x:expr) => {
            match $x {
                Ok(result) => result,
                Err(_) => {
                    return; // we expect the test to panic, so this will cause a test failure
                }
            }
        };
    }

    fn test_trie(root_hash: Digest, tries: Vec<HashedTestTrie>) {
        let context = return_on_err!(LmdbTestContext::new(&tries));
        let txn = return_on_err!(context.environment.create_read_txn());
        let _tmp = operations::keys::<TestKey, TestValue, _, _>(&txn, &context.store, &root_hash)
            .collect::<Vec<_>>();
    }

    #[test]
    #[should_panic]
    fn should_panic_on_leaf_after_extension() {
        let (root_hash, tries) = return_on_err!(create_invalid_extension_trie());
        test_trie(root_hash, tries);
    }

    #[test]
    #[should_panic]
    fn should_panic_when_key_not_matching_path() {
        let (root_hash, tries) = return_on_err!(create_invalid_path_trie());
        test_trie(root_hash, tries);
    }

    #[test]
    #[should_panic]
    fn should_panic_on_pointer_to_nonexisting_hash() {
        let (root_hash, tries) = return_on_err!(create_invalid_hash_trie());
        test_trie(root_hash, tries);
    }
}

mod keys_with_prefix_iterator {
    use crate::global_state::{
        transaction_source::TransactionSource,
        trie::Trie,
        trie_store::operations::{
            self,
            tests::{create_6_leaf_trie, LmdbTestContext, TestKey, TestValue, TEST_LEAVES},
        },
    };

    fn expected_keys(prefix: &[u8]) -> Vec<TestKey> {
        let mut tmp = TEST_LEAVES
            .iter()
            .filter_map(Trie::key)
            .filter(|key| key.0.starts_with(prefix))
            .cloned()
            .collect::<Vec<TestKey>>();
        tmp.sort();
        tmp
    }

    fn test_prefix(prefix: &[u8]) {
        let (root_hash, tries) = create_6_leaf_trie().expect("should create a trie");
        let context = LmdbTestContext::new(&tries).expect("should create a new context");
        let txn = context
            .environment
            .create_read_txn()
            .expect("should create a read txn");
        let expected = expected_keys(prefix);
        let mut actual = operations::keys_with_prefix::<TestKey, TestValue, _, _>(
            &txn,
            &context.store,
            &root_hash,
            prefix,
        )
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
        actual.sort();
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_prefixes() {
        test_prefix(&[]); // 6 leaves
        test_prefix(&[0]); // 6 leaves
        test_prefix(&[0, 1]); // 1 leaf
        test_prefix(&[0, 1, 0]); // 1 leaf
        test_prefix(&[0, 1, 1]); // 0 leaves
        test_prefix(&[0, 0]); // 5 leaves
        test_prefix(&[0, 0, 1]); // 0 leaves
        test_prefix(&[0, 0, 2]); // 1 leaf
        test_prefix(&[0, 0, 0, 0]); // 3 leaves, prefix points to an Extension
        test_prefix(&[0, 0, 0, 0, 0]); // 3 leaves
        test_prefix(&[0, 0, 0, 0, 0, 0]); // 2 leaves
        test_prefix(&[0, 0, 0, 0, 0, 0, 1]); // 1 leaf
    }
}
