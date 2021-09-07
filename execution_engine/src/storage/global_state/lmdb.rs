use std::{ops::Deref, sync::Arc};

use crate::shared::{additive_map::AdditiveMap, newtypes::CorrelationId, transform::Transform};
use casper_types::{Key, StoredValue};
use hashing::Digest;

use crate::storage::{
    error,
    global_state::{commit, StateProvider, StateReader},
    store::Store,
    transaction_source::{lmdb::LmdbEnvironment, Transaction, TransactionSource},
    trie::{merkle_proof::TrieMerkleProof, operations::create_hashed_empty_trie, Trie},
    trie_store::{
        lmdb::LmdbTrieStore,
        operations::{
            keys_with_prefix, missing_trie_keys, put_trie, read, read_with_proof, ReadResult,
        },
    },
};

/// Global state implemented against LMDB as a backing data store.
pub struct LmdbGlobalState {
    /// Environment for LMDB.
    pub(crate) environment: Arc<LmdbEnvironment>,
    /// Trie store held within LMDB.
    pub(crate) trie_store: Arc<LmdbTrieStore>,
    // TODO: make this a lazy-static
    /// Empty root hash used for a new trie.
    pub(crate) empty_root_hash: Digest,
}

/// Represents a "view" of global state at a particular root hash.
pub struct LmdbGlobalStateView {
    /// Environment for LMDB.
    pub(crate) environment: Arc<LmdbEnvironment>,
    /// Trie store held within LMDB.
    pub(crate) store: Arc<LmdbTrieStore>,
    /// Root hash of this "view".
    pub(crate) root_hash: Digest,
}

impl LmdbGlobalState {
    /// Creates an empty state from an existing environment and trie_store.
    pub fn empty(
        environment: Arc<LmdbEnvironment>,
        trie_store: Arc<LmdbTrieStore>,
    ) -> Result<Self, error::Error> {
        let root_hash: Digest = {
            let (root_hash, root) = create_hashed_empty_trie::<Key, StoredValue>()?;
            let mut txn = environment.create_read_write_txn()?;
            trie_store.put(&mut txn, &root_hash, &root)?;
            txn.commit()?;
            root_hash
        };
        Ok(LmdbGlobalState::new(environment, trie_store, root_hash))
    }

    /// Creates a state from an existing environment, store, and root_hash.
    /// Intended to be used for testing.
    pub(crate) fn new(
        environment: Arc<LmdbEnvironment>,
        trie_store: Arc<LmdbTrieStore>,
        empty_root_hash: Digest,
    ) -> Self {
        LmdbGlobalState {
            environment,
            trie_store,
            empty_root_hash,
        }
    }
}

impl StateReader<Key, StoredValue> for LmdbGlobalStateView {
    type Error = error::Error;

    fn read(
        &self,
        correlation_id: CorrelationId,
        key: &Key,
    ) -> Result<Option<StoredValue>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let ret = match read::<Key, StoredValue, lmdb::RoTransaction, LmdbTrieStore, Self::Error>(
            correlation_id,
            &txn,
            self.store.deref(),
            &self.root_hash,
            key,
        )? {
            ReadResult::Found(value) => Some(value),
            ReadResult::NotFound => None,
            ReadResult::RootNotFound => panic!("LmdbGlobalState has invalid root"),
        };
        txn.commit()?;
        Ok(ret)
    }

    fn read_with_proof(
        &self,
        correlation_id: CorrelationId,
        key: &Key,
    ) -> Result<Option<TrieMerkleProof<Key, StoredValue>>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let ret = match read_with_proof::<
            Key,
            StoredValue,
            lmdb::RoTransaction,
            LmdbTrieStore,
            Self::Error,
        >(
            correlation_id,
            &txn,
            self.store.deref(),
            &self.root_hash,
            key,
        )? {
            ReadResult::Found(value) => Some(value),
            ReadResult::NotFound => None,
            ReadResult::RootNotFound => panic!("LmdbGlobalState has invalid root"),
        };
        txn.commit()?;
        Ok(ret)
    }

    fn keys_with_prefix(
        &self,
        correlation_id: CorrelationId,
        prefix: &[u8],
    ) -> Result<Vec<Key>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let keys_iter = keys_with_prefix::<Key, StoredValue, _, _>(
            correlation_id,
            &txn,
            self.store.deref(),
            &self.root_hash,
            prefix,
        );
        let mut ret = Vec::new();
        for result in keys_iter {
            match result {
                Ok(key) => ret.push(key),
                Err(error) => return Err(error),
            }
        }
        txn.commit()?;
        Ok(ret)
    }
}

impl StateProvider for LmdbGlobalState {
    type Error = error::Error;

    type Reader = LmdbGlobalStateView;

    fn checkout(&self, state_hash: Digest) -> Result<Option<Self::Reader>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let maybe_root: Option<Trie<Key, StoredValue>> = self.trie_store.get(&txn, &state_hash)?;
        let maybe_state = maybe_root.map(|_| LmdbGlobalStateView {
            environment: Arc::clone(&self.environment),
            store: Arc::clone(&self.trie_store),
            root_hash: state_hash,
        });
        txn.commit()?;
        Ok(maybe_state)
    }

    fn commit(
        &self,
        correlation_id: CorrelationId,
        prestate_hash: Digest,
        effects: AdditiveMap<Key, Transform>,
    ) -> Result<Digest, Self::Error> {
        commit::<LmdbEnvironment, LmdbTrieStore, _, Self::Error>(
            &self.environment,
            &self.trie_store,
            correlation_id,
            prestate_hash,
            effects,
        )
        .map_err(Into::into)
    }

    fn empty_root(&self) -> Digest {
        self.empty_root_hash
    }

    fn read_trie(
        &self,
        _correlation_id: CorrelationId,
        trie_key: &Digest,
    ) -> Result<Option<Trie<Key, StoredValue>>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let ret: Option<Trie<Key, StoredValue>> = self.trie_store.get(&txn, trie_key)?;
        txn.commit()?;
        Ok(ret)
    }

    fn put_trie(
        &self,
        correlation_id: CorrelationId,
        trie: &Trie<Key, StoredValue>,
    ) -> Result<Digest, Self::Error> {
        let mut txn = self.environment.create_read_write_txn()?;
        let trie_hash = put_trie::<
            Key,
            StoredValue,
            lmdb::RwTransaction,
            LmdbTrieStore,
            Self::Error,
        >(correlation_id, &mut txn, &self.trie_store, trie)?;
        txn.commit()?;
        Ok(trie_hash)
    }

    /// Finds all of the keys of missing descendant `Trie<K,V>` values
    fn missing_trie_keys(
        &self,
        correlation_id: CorrelationId,
        trie_keys: Vec<Digest>,
    ) -> Result<Vec<Digest>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let missing_descendants =
            missing_trie_keys::<Key, StoredValue, lmdb::RoTransaction, LmdbTrieStore, Self::Error>(
                correlation_id,
                &txn,
                self.trie_store.deref(),
                trie_keys,
            )?;
        txn.commit()?;
        Ok(missing_descendants)
    }
}

#[cfg(test)]
mod tests {
    use lmdb::DatabaseFlags;
    use tempfile::tempdir;

    use casper_types::{account::AccountHash, CLValue};
    use hashing::Digest;

    use super::*;
    use crate::storage::{
        trie_store::operations::{write, WriteResult},
        DEFAULT_TEST_MAX_DB_SIZE, DEFAULT_TEST_MAX_READERS,
    };

    #[derive(Debug, Clone)]
    struct TestPair {
        key: Key,
        value: StoredValue,
    }

    fn create_test_pairs() -> [TestPair; 2] {
        [
            TestPair {
                key: Key::Account(AccountHash::new([1_u8; 32])),
                value: StoredValue::CLValue(CLValue::from_t(1_i32).unwrap()),
            },
            TestPair {
                key: Key::Account(AccountHash::new([2_u8; 32])),
                value: StoredValue::CLValue(CLValue::from_t(2_i32).unwrap()),
            },
        ]
    }

    fn create_test_pairs_updated() -> [TestPair; 3] {
        [
            TestPair {
                key: Key::Account(AccountHash::new([1u8; 32])),
                value: StoredValue::CLValue(CLValue::from_t("one".to_string()).unwrap()),
            },
            TestPair {
                key: Key::Account(AccountHash::new([2u8; 32])),
                value: StoredValue::CLValue(CLValue::from_t("two".to_string()).unwrap()),
            },
            TestPair {
                key: Key::Account(AccountHash::new([3u8; 32])),
                value: StoredValue::CLValue(CLValue::from_t(3_i32).unwrap()),
            },
        ]
    }

    fn create_test_state() -> (LmdbGlobalState, Digest) {
        let correlation_id = CorrelationId::new();
        let temp_dir = tempdir().unwrap();
        let environment = Arc::new(
            LmdbEnvironment::new(
                &temp_dir.path().to_path_buf(),
                DEFAULT_TEST_MAX_DB_SIZE,
                DEFAULT_TEST_MAX_READERS,
                true,
            )
            .unwrap(),
        );
        let trie_store =
            Arc::new(LmdbTrieStore::new(&environment, None, DatabaseFlags::empty()).unwrap());

        let ret = LmdbGlobalState::empty(environment, trie_store).unwrap();
        let mut current_root = ret.empty_root_hash;
        {
            let mut txn = ret.environment.create_read_write_txn().unwrap();

            for TestPair { key, value } in &create_test_pairs() {
                match write::<_, _, _, LmdbTrieStore, error::Error>(
                    correlation_id,
                    &mut txn,
                    &ret.trie_store,
                    &current_root,
                    key,
                    value,
                )
                .unwrap()
                {
                    WriteResult::Written(root_hash) => {
                        current_root = root_hash;
                    }
                    WriteResult::AlreadyExists => (),
                    WriteResult::RootNotFound => panic!("LmdbGlobalState has invalid root"),
                }
            }

            txn.commit().unwrap();
        }
        (ret, current_root)
    }

    #[test]
    fn reads_from_a_checkout_return_expected_values() {
        let correlation_id = CorrelationId::new();
        let (state, root_hash) = create_test_state();
        let checkout = state.checkout(root_hash).unwrap().unwrap();
        for TestPair { key, value } in create_test_pairs().iter().cloned() {
            assert_eq!(Some(value), checkout.read(correlation_id, &key).unwrap());
        }
    }

    #[test]
    fn checkout_fails_if_unknown_hash_is_given() {
        let (state, _) = create_test_state();
        let fake_hash: Digest = Digest::hash(&[1u8; 32]);
        let result = state.checkout(fake_hash).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn commit_updates_state() {
        let correlation_id = CorrelationId::new();
        let test_pairs_updated = create_test_pairs_updated();

        let (state, root_hash) = create_test_state();

        let effects: AdditiveMap<Key, Transform> = {
            let mut tmp = AdditiveMap::new();
            for TestPair { key, value } in &test_pairs_updated {
                tmp.insert(*key, Transform::Write(value.to_owned()));
            }
            tmp
        };

        let updated_hash = state.commit(correlation_id, root_hash, effects).unwrap();

        let updated_checkout = state.checkout(updated_hash).unwrap().unwrap();

        for TestPair { key, value } in test_pairs_updated.iter().cloned() {
            assert_eq!(
                Some(value),
                updated_checkout.read(correlation_id, &key).unwrap()
            );
        }
    }

    #[test]
    fn commit_updates_state_and_original_state_stays_intact() {
        let correlation_id = CorrelationId::new();
        let test_pairs_updated = create_test_pairs_updated();

        let (state, root_hash) = create_test_state();

        let effects: AdditiveMap<Key, Transform> = {
            let mut tmp = AdditiveMap::new();
            for TestPair { key, value } in &test_pairs_updated {
                tmp.insert(*key, Transform::Write(value.to_owned()));
            }
            tmp
        };

        let updated_hash = state.commit(correlation_id, root_hash, effects).unwrap();

        let updated_checkout = state.checkout(updated_hash).unwrap().unwrap();
        for TestPair { key, value } in test_pairs_updated.iter().cloned() {
            assert_eq!(
                Some(value),
                updated_checkout.read(correlation_id, &key).unwrap()
            );
        }

        let original_checkout = state.checkout(root_hash).unwrap().unwrap();
        for TestPair { key, value } in create_test_pairs().iter().cloned() {
            assert_eq!(
                Some(value),
                original_checkout.read(correlation_id, &key).unwrap()
            );
        }
        assert_eq!(
            None,
            original_checkout
                .read(correlation_id, &test_pairs_updated[2].key)
                .unwrap()
        );
    }
}
