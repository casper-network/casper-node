use std::{
    collections::HashMap,
    mem,
    sync::{Arc, RwLock},
};

use tracing::error;

use casper_hashing::Digest;
use casper_types::{Key, StoredValue};

use crate::global_state::{
    shared::{transform::Transform, AdditiveMap, CorrelationId},
    storage::{
        error,
        state::{CommitError, CommitProvider, StateProvider, StateReader},
        store::Store,
        transaction_source::{lmdb::LmdbEnvironment, Transaction, TransactionSource},
        trie::{merkle_proof::TrieMerkleProof, Trie, TrieRaw},
        trie_store::{
            lmdb::LmdbTrieStore,
            operations::{
                keys_with_prefix, missing_children, put_trie, read, read_with_proof, ReadResult,
            },
        },
    },
};

use super::lmdb::{LmdbGlobalState, LmdbGlobalStateView};

type SharedCache = Arc<RwLock<Cache>>;

pub struct Cache {
    cached_values: HashMap<Key, (bool, StoredValue)>,
}

impl Cache {
    fn new() -> Self {
        Cache {
            cached_values: HashMap::new(),
        }
    }

    fn insert_write(&mut self, key: Key, value: StoredValue) {
        self.cached_values.insert(key, (true, value));
    }

    fn insert_read(&mut self, key: Key, value: StoredValue) {
        self.cached_values.entry(key).or_insert((false, value));
    }

    fn get(&self, key: &Key) -> Option<&StoredValue> {
        self.cached_values.get(key).map(|(_dirty, value)| value)
    }

    /// Consumes self and returns only written values as values that were only read must be filtered
    /// out to prevent unnecessary writes.
    pub fn into_dirty_writes(self) -> HashMap<Key, StoredValue> {
        self.cached_values
            .into_iter()
            .filter_map(|(key, (dirty, value))| if dirty { Some((key, value)) } else { None })
            .collect()
    }
}

/// Global state implemented against LMDB as a backing data store.
#[derive(Clone)]
pub struct ScratchGlobalState {
    /// Underlying, cached stored values.
    cache: SharedCache,
    /// Underlying uncached global state which is delegated to for reads.
    state: Arc<LmdbGlobalState>,
}

/// Represents a "view" of global state at a particular root hash.
#[derive(Clone)]
pub struct ScratchGlobalStateView {
    /// tnderlying cached stored values.
    cache: SharedCache,
    /// Underlying uncached global state which is delegated to for reads.
    view: Arc<LmdbGlobalStateView>,
}

impl ScratchGlobalState {
    /// Creates a state from an existing environment, store, and root_hash.
    /// Intended to be used for testing.
    pub fn new(state: Arc<LmdbGlobalState>) -> Self {
        ScratchGlobalState {
            cache: Arc::new(RwLock::new(Cache::new())),
            state,
        }
    }

    // Write cached changes to the underlying data store.
    pub fn write_to_disk(&self, prestate_hash: Digest) -> Result<Digest, error::Error> {
        let cache = self.take_cache();
        let writes = cache.into_dirty_writes();
        let new_state_root =
            self.state
                .put_stored_values(CorrelationId::new(), prestate_hash, writes)?;
        if self.state.environment().is_manual_sync_enabled() {
            self.state.environment().sync()?
        }
        Ok(new_state_root)
    }

    pub fn take_cache(&self) -> Cache {
        let cache = mem::replace(&mut *self.cache.write().unwrap(), Cache::new());
        cache
    }

    pub fn lmdb(&self) -> &LmdbGlobalState {
        self.state.as_ref()
    }
}

impl StateReader<Key, StoredValue> for ScratchGlobalStateView {
    type Error = error::Error;

    fn read(
        &self,
        correlation_id: CorrelationId,
        key: &Key,
    ) -> Result<Option<StoredValue>, Self::Error> {
        if let Some(value) = self.cache.read().unwrap().get(key) {
            return Ok(Some(value.clone()));
        }
        let ret = self.view.read(correlation_id, key)?;
        if let Some(value) = ret.as_ref() {
            self.cache
                .write()
                .map_err(|_| error::Error::Poison)?
                .insert_read(*key, value.clone());
        }
        Ok(ret)
    }

    fn read_with_proof(
        &self,
        correlation_id: CorrelationId,
        key: &Key,
    ) -> Result<Option<TrieMerkleProof>, Self::Error> {
        self.view.read_with_proof(correlation_id, key)
    }

    fn keys_with_prefix(
        &self,
        correlation_id: CorrelationId,
        prefix: &[u8],
    ) -> Result<Vec<Key>, Self::Error> {
        self.view.keys_with_prefix(correlation_id, prefix)
    }
}

impl CommitProvider for ScratchGlobalState {
    /// State hash returned is the one provided, as we do not write to lmdb with this kind of global
    /// state. Note that the state hash is NOT used, and simply passed back to the caller.
    fn commit(
        &self,
        correlation_id: CorrelationId,
        state_hash: Digest,
        effects: AdditiveMap<Key, Transform>,
    ) -> Result<Digest, Self::Error> {
        let scratch_trie = self.state.get_scratch_store();
        for (key, transform) in effects.into_iter() {
            let cached_value = self.cache.read().unwrap().get(&key).cloned();
            let value = match (cached_value, transform) {
                (None, Transform::Write(new_value)) => new_value,
                (None, transform) => {
                    // It might be the case that for `Add*` operations we don't have the previous
                    // value in cache yet.
                    match operations::read::<_, _, Self::Error>(
                        correlation_id,
                        &scratch_trie,
                        &scratch_trie,
                        &state_hash,
                        &key,
                    )? {
                        ReadResult::Found(current_value) => {
                            match transform.apply(current_value.clone()) {
                                Ok(updated_value) => updated_value,
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
                    }
                }
                (Some(current_value), transform) => match transform.apply(current_value.clone()) {
                    Ok(updated_value) => updated_value,
                    Err(err) => {
                        error!(?key, ?err, "Key found, but could not apply transform");
                        return Err(CommitError::TransformError(err).into());
                    }
                },
            };

            self.cache.write().unwrap().insert_write(key, value);
        }
        Ok(state_hash)
    }
}

impl StateProvider for ScratchGlobalState {
    type Error = error::Error;

    type Reader = ScratchGlobalStateView;

    fn checkout(&self, state_hash: Digest) -> Result<Option<Self::Reader>, Self::Error> {
        let maybe_view = self.state.checkout(state_hash)?;
        let maybe_state = maybe_view.map(|view| ScratchGlobalStateView {
            cache: Arc::clone(&self.cache),
            view: Arc::new(view),
        });
        Ok(maybe_state)
    }

    fn empty_root(&self) -> Digest {
        self.state.empty_root_hash
    }

    fn get_trie_full(
        &self,
        correlation_id: CorrelationId,
        trie_key: &Digest,
    ) -> Result<Option<Bytes>, Self::Error> {
        self.state.get_trie_full(correlation_id, trie_key)
    }

    fn put_trie(&self, correlation_id: CorrelationId, trie: &[u8]) -> Result<Digest, Self::Error> {
        // ScratchGlobalState should always write tries directly to disk.
        self.state.put_trie(correlation_id, trie)
    }

    /// Finds all of the keys of missing directly descendant `Trie<K,V>` values
    fn missing_children(
        &self,
        correlation_id: CorrelationId,
        trie_raw: &[u8],
    ) -> Result<Vec<Digest>, Self::Error> {
        self.state.missing_children(correlation_id, trie_keys)
    }
}

#[cfg(test)]
mod tests {
    use lmdb::{DatabaseFlags, Transaction};
    use tempfile::tempdir;

    use casper_hashing::Digest;
    use casper_types::{account::AccountHash, CLValue};

    use crate::global_state::storage::{
        transaction_source::{lmdb::LmdbEnvironment, TransactionSource},
        trie_store::{lmdb::LmdbTrieStore, operations::WriteResult},
        DEFAULT_TEST_MAX_DB_SIZE, DEFAULT_TEST_MAX_READERS,
    };

    use super::*;

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

    fn create_test_transforms() -> AdditiveMap<Key, Transform> {
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

    struct TestState {
        state: LmdbGlobalState,
        root_hash: Digest,
    }

    fn create_test_state() -> TestState {
        let correlation_id = CorrelationId::new();
        let temp_dir = tempdir().unwrap();
        let environment = Arc::new(
            LmdbEnvironment::new(
                &temp_dir.path(),
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
                match operations::write::<_, LmdbTrieStore, error::Error>(
                    correlation_id,
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
        let correlation_id = CorrelationId::new();
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

        let scratch_root_hash = scratch
            .commit(correlation_id, root_hash, effects.clone())
            .unwrap();

        assert_eq!(
            scratch_root_hash, root_hash,
            "ScratchGlobalState should not modify the state root, as it does no hashing"
        );

        let lmdb_hash = state.commit(correlation_id, root_hash, effects).unwrap();
        let updated_checkout = state.checkout(lmdb_hash).unwrap().unwrap();

        let all_keys = updated_checkout
            .keys_with_prefix(correlation_id, &[])
            .unwrap();

        let stored_values = scratch.take_cache().into_dirty_writes();
        assert_eq!(all_keys.len(), stored_values.len());

        for key in all_keys {
            assert!(stored_values.get(&key).is_some());
            assert_eq!(
                stored_values.get(&key),
                updated_checkout
                    .read(correlation_id, &key)
                    .unwrap()
                    .as_ref()
            );
        }

        for TestPair { key, value } in test_pairs_updated.iter().cloned() {
            assert_eq!(
                Some(value),
                updated_checkout.read(correlation_id, &key).unwrap()
            );
        }
    }

    #[test]
    fn commit_updates_state_with_add() {
        let correlation_id = CorrelationId::new();
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
        scratch
            .commit(correlation_id, root_hash, effects.clone())
            .unwrap();
        let updated_hash = state2
            .commit(correlation_id, state_2_root_hash, effects)
            .unwrap();

        // Create add transforms as well
        let add_effects = create_test_transforms();
        scratch
            .commit(correlation_id, root_hash, add_effects.clone())
            .unwrap();
        let updated_hash = state2
            .commit(correlation_id, updated_hash, add_effects)
            .unwrap();

        let scratch_checkout = scratch.checkout(root_hash).unwrap().unwrap();
        let updated_checkout = state2.checkout(updated_hash).unwrap().unwrap();
        let all_keys = updated_checkout
            .keys_with_prefix(correlation_id, &[])
            .unwrap();

        // Check that cache matches the contents of the second instance of lmdb
        for key in all_keys {
            assert_eq!(
                scratch_checkout
                    .read(correlation_id, &key)
                    .unwrap()
                    .as_ref(),
                updated_checkout
                    .read(correlation_id, &key)
                    .unwrap()
                    .as_ref()
            );
        }
    }

    #[test]
    fn commit_updates_state_and_original_state_stays_intact() {
        let correlation_id = CorrelationId::new();
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

        let updated_hash = scratch.commit(correlation_id, root_hash, effects).unwrap();

        let updated_checkout = scratch.checkout(updated_hash).unwrap().unwrap();
        for TestPair { key, value } in test_pairs_updated.iter().cloned() {
            assert_eq!(
                Some(value),
                updated_checkout.read(correlation_id, &key).unwrap(),
                "ScratchGlobalState should not yet be written to the underlying lmdb state"
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
