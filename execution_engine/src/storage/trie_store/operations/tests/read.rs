//! This module contains tests for [`StateReader::read`].
//!
//! Our primary goal here is to test this functionality in isolation.
//! Therefore, we manually construct test tries from a well-known set of
//! leaves called [`TEST_LEAVES`](super::TEST_LEAVES), each of which represents a value we are
//! trying to store in the trie at a given key.
//!
//! We use two strategies for testing.  See the [`partial_tries`] and
//! [`full_tries`] modules for more info.

use super::*;
use crate::storage::error::{self, in_memory};

mod partial_tries {
    //! Here we construct 6 separate "partial" tries, increasing in size
    //! from 0 to 5 leaves.  Each of these tries contains no past history,
    //! only a single a root to read from.  The tests check that we can read
    //! only the expected set of leaves from the trie from this single root.

    use super::*;

    #[test]
    fn lmdb_reads_from_n_leaf_partial_trie_had_expected_results() {
        for (num_leaves, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = generator().unwrap();
            let context = LmdbTestContext::new(&tries).unwrap();
            let test_leaves = TEST_LEAVES;
            let (used, unused) = test_leaves.split_at(num_leaves);

            check_leaves::<_, _, _, _, error::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &root_hash,
                used,
                unused,
            )
            .unwrap();
        }
    }

    #[test]
    fn in_memory_reads_from_n_leaf_partial_trie_had_expected_results() {
        for (num_leaves, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let correlation_id = CorrelationId::new();
            let (root_hash, tries) = generator().unwrap();
            let context = InMemoryTestContext::new(&tries).unwrap();
            let test_leaves = TEST_LEAVES;
            let (used, unused) = test_leaves.split_at(num_leaves);

            check_leaves::<_, _, _, _, in_memory::Error>(
                correlation_id,
                &context.environment,
                &context.store,
                &root_hash,
                used,
                unused,
            )
            .unwrap();
        }
    }
}

mod full_tries {
    //! Here we construct a series of 6 "full" tries, increasing in size
    //! from 0 to 5 leaves.  Each trie contains the history from preceding
    //! tries in this series, and past history can be read from the roots of
    //! each preceding trie.  The tests check that we can read only the
    //! expected set of leaves from the trie at the current root and all past
    //! roots.

    use super::*;

    #[test]
    fn lmdb_reads_from_n_leaf_full_trie_had_expected_results() {
        let correlation_id = CorrelationId::new();
        let context = LmdbTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for (state_index, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            for (num_leaves, state) in states[..state_index].iter().enumerate() {
                let test_leaves = TEST_LEAVES;
                let (used, unused) = test_leaves.split_at(num_leaves);
                check_leaves::<_, _, _, _, error::Error>(
                    correlation_id,
                    &context.environment,
                    &context.store,
                    state,
                    used,
                    unused,
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn in_memory_reads_from_n_leaf_full_trie_had_expected_results() {
        let correlation_id = CorrelationId::new();
        let context = InMemoryTestContext::new(EMPTY_HASHED_TEST_TRIES).unwrap();
        let mut states: Vec<Digest> = Vec::new();

        for (state_index, generator) in TEST_TRIE_GENERATORS.iter().enumerate() {
            let (root_hash, tries) = generator().unwrap();
            context.update(&tries).unwrap();
            states.push(root_hash);

            for (num_leaves, state) in states[..state_index].iter().enumerate() {
                let test_leaves = TEST_LEAVES;
                let (used, unused) = test_leaves.split_at(num_leaves);
                check_leaves::<_, _, _, _, in_memory::Error>(
                    correlation_id,
                    &context.environment,
                    &context.store,
                    state,
                    used,
                    unused,
                )
                .unwrap();
            }
        }
    }
}
