use std::{collections::HashMap, ops::Deref, sync::Arc};

use lmdb::DatabaseFlags;

use tempfile::TempDir;

use crate::tracking_copy::TrackingCopy;
use casper_types::{
    execution::{Effects, Transform, TransformKind},
    Digest, Key, StoredValue,
};

use crate::global_state::{
    error::Error as GlobalStateError,
    state::{
        commit, put_stored_values, scratch::ScratchGlobalState, CommitProvider, StateProvider,
        StateReader,
    },
    store::Store,
    transaction_source::{lmdb::LmdbEnvironment, Transaction, TransactionSource},
    trie::{merkle_proof::TrieMerkleProof, operations::create_hashed_empty_trie, Trie, TrieRaw},
    trie_store::{
        lmdb::{LmdbTrieStore, ScratchTrieStore},
        operations::{
            keys_with_prefix, missing_children, prune, put_trie, read, read_with_proof,
            PruneResult, ReadResult,
        },
    },
    DEFAULT_MAX_DB_SIZE, DEFAULT_MAX_QUERY_DEPTH, DEFAULT_MAX_READERS,
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
    /// Max query depth
    pub max_query_depth: u64,
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
        max_query_depth: u64,
    ) -> Result<Self, GlobalStateError> {
        let root_hash: Digest = {
            let (root_hash, root) = compute_empty_root_hash()?;
            let mut txn = environment.create_read_write_txn()?;
            trie_store.put(&mut txn, &root_hash, &root)?;
            txn.commit()?;
            environment.env().sync(true)?;
            root_hash
        };
        Ok(LmdbGlobalState::new(
            environment,
            trie_store,
            root_hash,
            max_query_depth,
        ))
    }

    /// Creates a state from an existing environment, store, and root_hash.
    /// Intended to be used for testing.
    pub fn new(
        environment: Arc<LmdbEnvironment>,
        trie_store: Arc<LmdbTrieStore>,
        empty_root_hash: Digest,
        max_query_depth: u64,
    ) -> Self {
        LmdbGlobalState {
            environment,
            trie_store,
            empty_root_hash,
            max_query_depth,
        }
    }

    /// Creates an in-memory cache for changes written.
    pub fn create_scratch(&self) -> ScratchGlobalState {
        ScratchGlobalState::new(
            Arc::clone(&self.environment),
            Arc::clone(&self.trie_store),
            self.empty_root_hash,
            self.max_query_depth,
        )
    }

    /// Write stored values to LMDB.
    pub fn put_stored_values(
        &self,
        prestate_hash: Digest,
        stored_values: HashMap<Key, StoredValue>,
    ) -> Result<Digest, GlobalStateError> {
        let scratch_trie = self.get_scratch_store();
        let new_state_root = put_stored_values::<_, _, GlobalStateError>(
            &scratch_trie,
            &scratch_trie,
            prestate_hash,
            stored_values,
        )?;
        scratch_trie.write_root_to_db(new_state_root)?;
        Ok(new_state_root)
    }

    /// Gets a scratch trie store.
    fn get_scratch_store(&self) -> ScratchTrieStore {
        ScratchTrieStore::new(Arc::clone(&self.trie_store), Arc::clone(&self.environment))
    }

    /// Get a reference to the lmdb global state's environment.
    #[must_use]
    pub fn environment(&self) -> &LmdbEnvironment {
        &self.environment
    }

    /// Get a reference to the lmdb global state's trie store.
    #[must_use]
    pub fn trie_store(&self) -> &LmdbTrieStore {
        &self.trie_store
    }

    /// Returns an initial, empty root hash of the underlying trie.
    pub fn empty_state_root_hash(&self) -> Digest {
        self.empty_root_hash
    }
}

fn compute_empty_root_hash() -> Result<(Digest, Trie<Key, StoredValue>), GlobalStateError> {
    let (root_hash, root) = create_hashed_empty_trie::<Key, StoredValue>()?;
    Ok((root_hash, root))
}

impl StateReader<Key, StoredValue> for LmdbGlobalStateView {
    type Error = GlobalStateError;

    fn read(&self, key: &Key) -> Result<Option<StoredValue>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let ret = match read::<Key, StoredValue, lmdb::RoTransaction, LmdbTrieStore, Self::Error>(
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
        key: &Key,
    ) -> Result<Option<TrieMerkleProof<Key, StoredValue>>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let ret = match read_with_proof::<
            Key,
            StoredValue,
            lmdb::RoTransaction,
            LmdbTrieStore,
            Self::Error,
        >(&txn, self.store.deref(), &self.root_hash, key)?
        {
            ReadResult::Found(value) => Some(value),
            ReadResult::NotFound => None,
            ReadResult::RootNotFound => panic!("LmdbGlobalState has invalid root"),
        };
        txn.commit()?;
        Ok(ret)
    }

    fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<Key>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let keys_iter = keys_with_prefix::<Key, StoredValue, _, _>(
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

impl CommitProvider for LmdbGlobalState {
    fn commit(&self, prestate_hash: Digest, effects: Effects) -> Result<Digest, GlobalStateError> {
        commit::<LmdbEnvironment, LmdbTrieStore, GlobalStateError>(
            &self.environment,
            &self.trie_store,
            prestate_hash,
            effects,
        )
        .map_err(Into::into)
    }
}

impl StateProvider for LmdbGlobalState {
    type Reader = LmdbGlobalStateView;

    fn checkout(&self, state_hash: Digest) -> Result<Option<Self::Reader>, GlobalStateError> {
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

    fn tracking_copy(
        &self,
        hash: Digest,
    ) -> Result<Option<TrackingCopy<Self::Reader>>, GlobalStateError> {
        match self.checkout(hash)? {
            Some(reader) => Ok(Some(TrackingCopy::new(reader, self.max_query_depth))),
            None => Ok(None),
        }
    }

    fn empty_root(&self) -> Digest {
        self.empty_root_hash
    }

    fn get_trie_full(&self, trie_key: &Digest) -> Result<Option<TrieRaw>, GlobalStateError> {
        let txn = self.environment.create_read_txn()?;
        let ret: Option<TrieRaw> =
            Store::<Digest, Trie<Digest, StoredValue>>::get_raw(&*self.trie_store, &txn, trie_key)?
                .map(TrieRaw::new);
        txn.commit()?;
        Ok(ret)
    }

    fn put_trie(&self, trie: &[u8]) -> Result<Digest, GlobalStateError> {
        let mut txn = self.environment.create_read_write_txn()?;
        let trie_hash =
            put_trie::<Key, StoredValue, lmdb::RwTransaction, LmdbTrieStore, GlobalStateError>(
                &mut txn,
                &self.trie_store,
                trie,
            )?;
        txn.commit()?;
        Ok(trie_hash)
    }

    /// Finds all of the keys of missing directly descendant `Trie<K,V>` values.
    fn missing_children(&self, trie_raw: &[u8]) -> Result<Vec<Digest>, GlobalStateError> {
        let txn = self.environment.create_read_txn()?;
        let missing_hashes = missing_children::<
            Key,
            StoredValue,
            lmdb::RoTransaction,
            LmdbTrieStore,
            GlobalStateError,
        >(&txn, self.trie_store.deref(), trie_raw)?;
        txn.commit()?;
        Ok(missing_hashes)
    }

    /// Prune keys.
    fn prune_keys(
        &self,
        mut state_root_hash: Digest,
        keys: &[Key],
    ) -> Result<PruneResult, GlobalStateError> {
        let scratch_trie_store = self.get_scratch_store();

        let mut txn = scratch_trie_store.create_read_write_txn()?;

        for key in keys {
            let prune_results = prune::<Key, StoredValue, _, _, GlobalStateError>(
                &mut txn,
                &scratch_trie_store,
                &state_root_hash,
                key,
            );
            match prune_results? {
                PruneResult::Pruned(root) => {
                    state_root_hash = root;
                }
                other => return Ok(other),
            }
        }

        txn.commit()?;

        scratch_trie_store.write_root_to_db(state_root_hash)?;
        Ok(PruneResult::Pruned(state_root_hash))
    }
}

/// Creates prepopulated LMDB global state instance that stores data in a temporary directory. As
/// soon as the `TempDir` instance is dropped all the data stored will be removed from the disk as
/// well.
pub fn make_temporary_global_state(
    initial_data: impl IntoIterator<Item = (Key, StoredValue)>,
) -> (LmdbGlobalState, Digest, TempDir) {
    let tempdir = tempfile::tempdir().expect("should create tempdir");

    let lmdb_global_state = {
        let lmdb_environment = LmdbEnvironment::new(
            tempdir.path(),
            DEFAULT_MAX_DB_SIZE,
            DEFAULT_MAX_READERS,
            false,
        )
        .expect("should create lmdb environment");
        let lmdb_trie_store = LmdbTrieStore::new(&lmdb_environment, None, DatabaseFlags::default())
            .expect("should create lmdb trie store");
        LmdbGlobalState::empty(
            Arc::new(lmdb_environment),
            Arc::new(lmdb_trie_store),
            DEFAULT_MAX_QUERY_DEPTH,
        )
        .expect("should create lmdb global state")
    };

    let mut root_hash = lmdb_global_state.empty_root_hash;

    let mut effects = Effects::new();

    for (key, stored_value) in initial_data {
        let transform = Transform::new(key.normalize(), TransformKind::Write(stored_value));
        effects.push(transform);
    }

    root_hash = lmdb_global_state
        .commit(root_hash, effects)
        .expect("Creation of account should be a success.");

    (lmdb_global_state, root_hash, tempdir)
}

#[cfg(test)]
mod tests {
    use casper_types::{account::AccountHash, execution::TransformKind, CLValue, Digest};

    use crate::global_state::state::scratch::tests::TestPair;

    use super::*;

    fn create_test_pairs() -> Vec<(Key, StoredValue)> {
        vec![
            (
                Key::Account(AccountHash::new([1_u8; 32])),
                StoredValue::CLValue(CLValue::from_t(1_i32).unwrap()),
            ),
            (
                Key::Account(AccountHash::new([2_u8; 32])),
                StoredValue::CLValue(CLValue::from_t(2_i32).unwrap()),
            ),
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

    #[test]
    fn reads_from_a_checkout_return_expected_values() {
        let test_pairs = create_test_pairs();
        let (state, root_hash, _tempdir) = make_temporary_global_state(test_pairs.clone());
        let checkout = state.checkout(root_hash).unwrap().unwrap();
        for (key, value) in test_pairs {
            assert_eq!(Some(value), checkout.read(&key).unwrap());
        }
    }

    #[test]
    fn checkout_fails_if_unknown_hash_is_given() {
        let (state, _, _tempdir) = make_temporary_global_state(create_test_pairs());
        let fake_hash: Digest = Digest::hash([1u8; 32]);
        let result = state.checkout(fake_hash).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn commit_updates_state() {
        let test_pairs_updated = create_test_pairs_updated();

        let (state, root_hash, _tempdir) = make_temporary_global_state(create_test_pairs());

        let effects = {
            let mut tmp = Effects::new();
            for TestPair { key, value } in &test_pairs_updated {
                let transform = Transform::new(*key, TransformKind::Write(value.clone()));
                tmp.push(transform);
            }
            tmp
        };

        let updated_hash = state.commit(root_hash, effects).unwrap();

        let updated_checkout = state.checkout(updated_hash).unwrap().unwrap();

        for TestPair { key, value } in test_pairs_updated.iter().cloned() {
            assert_eq!(Some(value), updated_checkout.read(&key).unwrap());
        }
    }

    #[test]
    fn commit_updates_state_and_original_state_stays_intact() {
        let test_pairs_updated = create_test_pairs_updated();

        let (state, root_hash, _tempdir) = make_temporary_global_state(create_test_pairs());

        let effects = {
            let mut tmp = Effects::new();
            for TestPair { key, value } in &test_pairs_updated {
                let transform = Transform::new(*key, TransformKind::Write(value.clone()));
                tmp.push(transform);
            }
            tmp
        };

        let updated_hash = state.commit(root_hash, effects).unwrap();

        let updated_checkout = state.checkout(updated_hash).unwrap().unwrap();
        for TestPair { key, value } in test_pairs_updated.iter().cloned() {
            assert_eq!(Some(value), updated_checkout.read(&key).unwrap());
        }

        let original_checkout = state.checkout(root_hash).unwrap().unwrap();
        for (key, value) in create_test_pairs().iter().cloned() {
            assert_eq!(Some(value), original_checkout.read(&key).unwrap());
        }
        assert_eq!(
            None,
            original_checkout.read(&test_pairs_updated[2].key).unwrap()
        );
    }
}
