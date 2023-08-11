use std::{
    collections::{HashMap, HashSet},
    mem,
    ops::Deref,
    sync::{Arc, RwLock},
};

use tracing::{debug, error};

use casper_types::{Digest, Key, StoredValue};

use crate::global_state::{
    error,
    shared::{
        transform::{Transform, TransformInstruction},
        AdditiveMap,
    },
    state::{CommitError, CommitProvider, StateProvider, StateReader},
    store::Store,
    transaction_source::{lmdb::LmdbEnvironment, Transaction, TransactionSource},
    trie::{merkle_proof::TrieMerkleProof, Trie, TrieRaw},
    trie_store::{
        lmdb::LmdbTrieStore,
        operations::{
            keys_with_prefix, missing_children, prune, put_trie, read, read_with_proof,
            PruneResult, ReadResult,
        },
    },
};

type SharedCache = Arc<RwLock<Cache>>;

struct Cache {
    cached_values: HashMap<Key, (bool, StoredValue)>,
    pruned: HashSet<Key>,
}

impl Cache {
    fn new() -> Self {
        Cache {
            cached_values: HashMap::new(),
            pruned: HashSet::new(),
        }
    }

    fn insert_write(&mut self, key: Key, value: StoredValue) {
        self.pruned.remove(&key);
        self.cached_values.insert(key, (true, value));
    }

    fn insert_read(&mut self, key: Key, value: StoredValue) {
        self.cached_values.entry(key).or_insert((false, value));
    }

    fn prune(&mut self, key: Key) {
        self.cached_values.remove(&key);
        self.pruned.insert(key);
    }

    fn get(&self, key: &Key) -> Option<&StoredValue> {
        if self.pruned.contains(key) {
            return None;
        }
        self.cached_values.get(key).map(|(_dirty, value)| value)
    }

    /// Consumes self and returns only written values as values that were only read must be filtered
    /// out to prevent unnecessary writes.
    fn into_dirty_writes(self) -> (HashMap<Key, StoredValue>, HashSet<Key>) {
        let keys_to_prune = self.pruned;
        let stored_values: HashMap<Key, StoredValue> = self
            .cached_values
            .into_iter()
            .filter_map(|(key, (dirty, value))| if dirty { Some((key, value)) } else { None })
            .collect();
        debug!(
            "Cache: into_dirty_writes: prune_count: {} store_count: {}",
            keys_to_prune.len(),
            stored_values.len()
        );
        (stored_values, keys_to_prune)
    }
}

/// Global state implemented against LMDB as a backing data store.
pub struct ScratchGlobalState {
    /// Underlying, cached stored values.
    cache: SharedCache,
    /// Environment for LMDB.
    pub(crate) environment: Arc<LmdbEnvironment>,
    /// Trie store held within LMDB.
    pub(crate) trie_store: Arc<LmdbTrieStore>,
    // TODO: make this a lazy-static
    /// Empty root hash used for a new trie.
    pub(crate) empty_root_hash: Digest,
}

/// Represents a "view" of global state at a particular root hash.
pub struct ScratchGlobalStateView {
    cache: SharedCache,
    /// Environment for LMDB.
    pub(crate) environment: Arc<LmdbEnvironment>,
    /// Trie store held within LMDB.
    pub(crate) trie_store: Arc<LmdbTrieStore>,
    /// Root hash of this "view".
    pub(crate) root_hash: Digest,
}

impl ScratchGlobalState {
    /// Creates a state from an existing environment, store, and root_hash.
    /// Intended to be used for testing.
    pub fn new(
        environment: Arc<LmdbEnvironment>,
        trie_store: Arc<LmdbTrieStore>,
        empty_root_hash: Digest,
    ) -> Self {
        ScratchGlobalState {
            cache: Arc::new(RwLock::new(Cache::new())),
            environment,
            trie_store,
            empty_root_hash,
        }
    }

    /// Consume self and return inner cache.
    pub fn into_inner(self) -> (HashMap<Key, StoredValue>, HashSet<Key>) {
        let cache = mem::replace(&mut *self.cache.write().unwrap(), Cache::new());
        cache.into_dirty_writes()
    }
}

impl StateReader<Key, StoredValue> for ScratchGlobalStateView {
    type Error = error::Error;

    fn read(&self, key: &Key) -> Result<Option<StoredValue>, Self::Error> {
        if self.cache.read().unwrap().pruned.contains(key) {
            return Ok(None);
        }
        if let Some(value) = self.cache.read().unwrap().get(key) {
            return Ok(Some(value.clone()));
        }
        let txn = self.environment.create_read_txn()?;
        let ret = match read::<Key, StoredValue, lmdb::RoTransaction, LmdbTrieStore, Self::Error>(
            &txn,
            self.trie_store.deref(),
            &self.root_hash,
            key,
        )? {
            ReadResult::Found(value) => {
                self.cache.write().unwrap().insert_read(*key, value.clone());
                Some(value)
            }
            ReadResult::NotFound => None,
            ReadResult::RootNotFound => panic!("ScratchGlobalState has invalid root"),
        };
        txn.commit()?;
        Ok(ret)
    }

    fn read_with_proof(
        &self,
        key: &Key,
    ) -> Result<Option<TrieMerkleProof<Key, StoredValue>>, Self::Error> {
        if self.cache.read().unwrap().pruned.contains(key) {
            return Ok(None);
        }
        let txn = self.environment.create_read_txn()?;
        let ret = match read_with_proof::<
            Key,
            StoredValue,
            lmdb::RoTransaction,
            LmdbTrieStore,
            Self::Error,
        >(&txn, self.trie_store.deref(), &self.root_hash, key)?
        {
            ReadResult::Found(value) => Some(value),
            ReadResult::NotFound => None,
            ReadResult::RootNotFound => panic!("LmdbWithCacheGlobalState has invalid root"),
        };
        txn.commit()?;
        Ok(ret)
    }

    fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<Key>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let keys_iter = keys_with_prefix::<Key, StoredValue, _, _>(
            &txn,
            self.trie_store.deref(),
            &self.root_hash,
            prefix,
        );
        let mut ret = Vec::new();
        for result in keys_iter {
            match result {
                Ok(key) => {
                    if !self.cache.read().unwrap().pruned.contains(&key) {
                        ret.push(key);
                    }
                }
                Err(error) => return Err(error),
            }
        }
        txn.commit()?;
        Ok(ret)
    }
}

impl CommitProvider for ScratchGlobalState {
    /// State hash returned is the one provided, as we do not write to lmdb with this kind of global
    /// state. Note that the state hash is NOT used, and simply passed back to the caller.
    fn commit(
        &self,
        state_hash: Digest,
        effects: AdditiveMap<Key, Transform>,
    ) -> Result<Digest, Self::Error> {
        for (key, transform) in effects.into_iter() {
            let cached_value = self.cache.read().unwrap().get(&key).cloned();
            let instruction = match (cached_value, transform) {
                (None, Transform::Write(new_value)) => TransformInstruction::store(new_value),
                (None, transform) => {
                    // It might be the case that for `Add*` operations we don't have the previous
                    // value in cache yet.
                    let txn = self.environment.create_read_txn()?;
                    let instruction = match read::<
                        Key,
                        StoredValue,
                        lmdb::RoTransaction,
                        LmdbTrieStore,
                        Self::Error,
                    >(
                        &txn, self.trie_store.deref(), &state_hash, &key
                    )? {
                        ReadResult::Found(current_value) => {
                            match transform.apply(current_value.clone()) {
                                Ok(instruction) => instruction,
                                Err(err) => {
                                    error!(?key, ?err, "Key found, but could not apply transform");
                                    return Err(CommitError::TransformError(err).into());
                                }
                            }
                        }
                        ReadResult::NotFound => {
                            error!(
                                ?key,
                                ?transform,
                                "Key not found while attempting to apply transform"
                            );
                            return Err(CommitError::KeyNotFound(key).into());
                        }
                        ReadResult::RootNotFound => {
                            error!(root_hash=?state_hash, "root not found");
                            return Err(CommitError::ReadRootNotFound(state_hash).into());
                        }
                    };
                    txn.commit()?;
                    instruction
                }
                (Some(current_value), transform) => match transform.apply(current_value.clone()) {
                    Ok(instruction) => instruction,
                    Err(err) => {
                        error!(?key, ?err, "Key found, but could not apply transform");
                        return Err(CommitError::TransformError(err).into());
                    }
                },
            };

            match instruction {
                TransformInstruction::Store(value) => {
                    self.cache.write().unwrap().insert_write(key, value);
                }
                TransformInstruction::Prune(key) => {
                    self.cache.write().unwrap().prune(key);
                }
            }
        }
        Ok(state_hash)
    }
}

impl StateProvider for ScratchGlobalState {
    type Error = error::Error;

    type Reader = ScratchGlobalStateView;

    fn checkout(&self, state_hash: Digest) -> Result<Option<Self::Reader>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let maybe_root: Option<Trie<Key, StoredValue>> = self.trie_store.get(&txn, &state_hash)?;
        let maybe_state = maybe_root.map(|_| ScratchGlobalStateView {
            cache: Arc::clone(&self.cache),
            environment: Arc::clone(&self.environment),
            trie_store: Arc::clone(&self.trie_store),
            root_hash: state_hash,
        });
        txn.commit()?;
        Ok(maybe_state)
    }

    fn empty_root(&self) -> Digest {
        self.empty_root_hash
    }

    fn get_trie_full(&self, trie_key: &Digest) -> Result<Option<TrieRaw>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let ret: Option<TrieRaw> =
            Store::<Digest, Trie<Digest, StoredValue>>::get_raw(&*self.trie_store, &txn, trie_key)?
                .map(TrieRaw::new);
        txn.commit()?;
        Ok(ret)
    }

    fn put_trie(&self, trie: &[u8]) -> Result<Digest, Self::Error> {
        let mut txn = self.environment.create_read_write_txn()?;
        let trie_hash =
            put_trie::<Key, StoredValue, lmdb::RwTransaction, LmdbTrieStore, Self::Error>(
                &mut txn,
                &self.trie_store,
                trie,
            )?;
        txn.commit()?;
        Ok(trie_hash)
    }

    /// Finds all of the keys of missing directly descendant `Trie<K,V>` values
    fn missing_children(&self, trie_raw: &[u8]) -> Result<Vec<Digest>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let missing_descendants =
            missing_children::<Key, StoredValue, lmdb::RoTransaction, LmdbTrieStore, Self::Error>(
                &txn,
                self.trie_store.deref(),
                trie_raw,
            )?;
        txn.commit()?;
        Ok(missing_descendants)
    }

    fn prune_keys(
        &self,
        mut state_root_hash: Digest,
        keys_to_delete: &[Key],
    ) -> Result<PruneResult, Self::Error> {
        let mut txn = self.environment.create_read_write_txn()?;
        for key in keys_to_delete {
            let prune_result = prune::<Key, StoredValue, _, _, Self::Error>(
                &mut txn,
                self.trie_store.deref(),
                &state_root_hash,
                key,
            );
            match prune_result? {
                PruneResult::Pruned(root) => {
                    state_root_hash = root;
                }
                other => return Ok(other),
            }
        }
        txn.commit()?;
        Ok(PruneResult::Pruned(state_root_hash))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use lmdb::DatabaseFlags;
    use tempfile::tempdir;

    use casper_types::{account::AccountHash, CLValue, Digest};

    use super::*;
    use crate::global_state::{
        state::{lmdb::LmdbGlobalState, CommitProvider},
        trie_store::operations::{write, WriteResult},
        DEFAULT_TEST_MAX_DB_SIZE, DEFAULT_TEST_MAX_READERS,
    };

    #[derive(Debug, Clone)]
    pub(crate) struct TestPair {
        pub key: Key,
        pub value: StoredValue,
    }

    pub(crate) fn create_test_pairs() -> [TestPair; 2] {
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

    pub(crate) fn create_test_pairs_updated() -> [TestPair; 3] {
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

    pub(crate) fn create_test_transforms() -> AdditiveMap<Key, Transform> {
        let mut transforms = AdditiveMap::new();
        transforms.insert(
            Key::Account(AccountHash::new([3u8; 32])),
            Transform::Write(StoredValue::CLValue(CLValue::from_t("one").unwrap())),
        );
        transforms.insert(
            Key::Account(AccountHash::new([3u8; 32])),
            Transform::AddInt32(1),
        );
        transforms.insert(
            Key::Account(AccountHash::new([3u8; 32])),
            Transform::AddInt32(2),
        );
        transforms
    }

    pub(crate) struct TestState {
        state: LmdbGlobalState,
        root_hash: Digest,
    }

    pub(crate) fn create_test_state() -> TestState {
        let temp_dir = tempdir().unwrap();
        let environment = Arc::new(
            LmdbEnvironment::new(
                temp_dir.path(),
                DEFAULT_TEST_MAX_DB_SIZE,
                DEFAULT_TEST_MAX_READERS,
                true,
            )
            .unwrap(),
        );
        let trie_store =
            Arc::new(LmdbTrieStore::new(&environment, None, DatabaseFlags::empty()).unwrap());

        let state = LmdbGlobalState::empty(environment, trie_store).unwrap();
        let mut current_root = state.empty_root_hash;
        {
            let mut txn = state.environment.create_read_write_txn().unwrap();

            for TestPair { key, value } in &create_test_pairs() {
                match write::<_, _, _, LmdbTrieStore, error::Error>(
                    &mut txn,
                    &state.trie_store,
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
                    WriteResult::RootNotFound => {
                        panic!("LmdbWithCacheGlobalState has invalid root")
                    }
                }
            }

            txn.commit().unwrap();
        }
        TestState {
            state,
            root_hash: current_root,
        }
    }

    #[test]
    fn commit_updates_state() {
        let test_pairs_updated = create_test_pairs_updated();

        let TestState { state, root_hash } = create_test_state();

        let scratch = state.create_scratch();

        let effects: AdditiveMap<Key, Transform> = {
            let mut tmp = AdditiveMap::new();
            for TestPair { key, value } in &test_pairs_updated {
                tmp.insert(*key, Transform::Write(value.to_owned()));
            }
            tmp
        };

        let scratch_root_hash = scratch.commit(root_hash, effects.clone()).unwrap();

        assert_eq!(
            scratch_root_hash, root_hash,
            "ScratchGlobalState should not modify the state root, as it does no hashing"
        );

        let lmdb_hash = state.commit(root_hash, effects).unwrap();
        let updated_checkout = state.checkout(lmdb_hash).unwrap().unwrap();

        let all_keys = updated_checkout.keys_with_prefix(&[]).unwrap();

        let (stored_values, _) = scratch.into_inner();
        assert_eq!(all_keys.len(), stored_values.len());

        for key in all_keys {
            assert!(stored_values.get(&key).is_some());
            assert_eq!(
                stored_values.get(&key),
                updated_checkout.read(&key).unwrap().as_ref()
            );
        }

        for TestPair { key, value } in test_pairs_updated.iter().cloned() {
            assert_eq!(Some(value), updated_checkout.read(&key).unwrap());
        }
    }

    #[test]
    fn commit_updates_state_with_add() {
        let test_pairs_updated = create_test_pairs_updated();

        // create two lmdb instances, with a scratch instance on the first
        let TestState { state, root_hash } = create_test_state();
        let TestState {
            state: state2,
            root_hash: state_2_root_hash,
        } = create_test_state();

        let scratch = state.create_scratch();

        let effects: AdditiveMap<Key, Transform> = {
            let mut tmp = AdditiveMap::new();
            for TestPair { key, value } in &test_pairs_updated {
                tmp.insert(*key, Transform::Write(value.to_owned()));
            }
            tmp
        };

        // Commit effects to both databases.
        scratch.commit(root_hash, effects.clone()).unwrap();
        let updated_hash = state2.commit(state_2_root_hash, effects).unwrap();

        // Create add transforms as well
        let add_effects = create_test_transforms();
        scratch.commit(root_hash, add_effects.clone()).unwrap();
        let updated_hash = state2.commit(updated_hash, add_effects).unwrap();

        let scratch_checkout = scratch.checkout(root_hash).unwrap().unwrap();
        let updated_checkout = state2.checkout(updated_hash).unwrap().unwrap();
        let all_keys = updated_checkout.keys_with_prefix(&[]).unwrap();

        // Check that cache matches the contents of the second instance of lmdb
        for key in all_keys {
            assert_eq!(
                scratch_checkout.read(&key).unwrap().as_ref(),
                updated_checkout.read(&key).unwrap().as_ref()
            );
        }
    }

    #[test]
    fn commit_updates_state_and_original_state_stays_intact() {
        let test_pairs_updated = create_test_pairs_updated();

        let TestState {
            state, root_hash, ..
        } = create_test_state();

        let scratch = state.create_scratch();

        let effects: AdditiveMap<Key, Transform> = {
            let mut tmp = AdditiveMap::new();
            for TestPair { key, value } in &test_pairs_updated {
                tmp.insert(*key, Transform::Write(value.to_owned()));
            }
            tmp
        };

        let updated_hash = scratch.commit(root_hash, effects).unwrap();

        let updated_checkout = scratch.checkout(updated_hash).unwrap().unwrap();
        for TestPair { key, value } in test_pairs_updated.iter().cloned() {
            assert_eq!(
                Some(value),
                updated_checkout.read(&key).unwrap(),
                "ScratchGlobalState should not yet be written to the underlying lmdb state"
            );
        }

        let original_checkout = state.checkout(root_hash).unwrap().unwrap();
        for TestPair { key, value } in create_test_pairs().iter().cloned() {
            assert_eq!(Some(value), original_checkout.read(&key).unwrap());
        }
        assert_eq!(
            None,
            original_checkout.read(&test_pairs_updated[2].key).unwrap()
        );
    }
}
