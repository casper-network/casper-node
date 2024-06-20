use lmdb::RwTransaction;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    mem,
    ops::Deref,
    sync::{Arc, RwLock},
};

use tracing::{debug, error};

use casper_types::{
    bytesrepr::{self, ToBytes},
    execution::{Effects, TransformInstruction, TransformKindV2, TransformV2},
    global_state::TrieMerkleProof,
    Digest, Key, StoredValue,
};

use crate::{
    data_access_layer::{
        FlushRequest, FlushResult, PutTrieRequest, PutTrieResult, TrieElement, TrieRequest,
        TrieResult,
    },
    global_state::{
        error::Error as GlobalStateError,
        state::{CommitError, CommitProvider, StateProvider, StateReader},
        store::Store,
        transaction_source::{lmdb::LmdbEnvironment, Transaction, TransactionSource},
        trie::{Trie, TrieRaw},
        trie_store::{
            lmdb::LmdbTrieStore,
            operations::{
                keys_with_prefix, missing_children, put_trie, read, read_with_proof, ReadResult,
            },
        },
    },
};

use crate::tracking_copy::TrackingCopy;

type SharedCache = Arc<RwLock<Cache>>;

struct Cache {
    cached_values: BTreeMap<Key, (bool, StoredValue)>,
    pruned: BTreeSet<Key>,
    cached_keys: CacheTrie<Key>,
}

use std::collections::HashMap;

struct CacheTrieNode<T> {
    children: HashMap<u8, CacheTrieNode<T>>,
    value: Option<T>,
}

impl<T> CacheTrieNode<T> {
    fn new() -> Self {
        CacheTrieNode {
            children: HashMap::new(),
            value: None,
        }
    }

    fn remove(&mut self, bytes: &[u8], depth: usize) -> bool {
        if depth == bytes.len() {
            if self.value.is_some() {
                self.value = None;
                return self.children.is_empty();
            }
            return false;
        }

        if let Some(child_node) = self.children.get_mut(&bytes[depth]) {
            if child_node.remove(bytes, depth + 1) {
                self.children.remove(&bytes[depth]);
                return self.value.is_none() && self.children.is_empty();
            }
        }
        false
    }
}

struct CacheTrie<T: Copy> {
    root: CacheTrieNode<T>,
}

impl<T: Copy> CacheTrie<T> {
    fn new() -> Self {
        CacheTrie {
            root: CacheTrieNode::new(),
        }
    }

    fn insert(&mut self, key_bytes: &[u8], key: T) {
        let mut current_node = &mut self.root;
        for &byte in key_bytes {
            current_node = current_node
                .children
                .entry(byte)
                .or_insert(CacheTrieNode::new());
        }
        current_node.value = Some(key);
    }

    fn keys_with_prefix(&self, prefix: &[u8]) -> Vec<T> {
        let mut current_node = &self.root;
        let mut result = Vec::new();

        for &byte in prefix {
            match current_node.children.get(&byte) {
                Some(node) => current_node = node,
                None => return result,
            }
        }

        self.collect_keys(current_node, &mut result);
        result
    }

    fn collect_keys(&self, start_node: &CacheTrieNode<T>, result: &mut Vec<T>) {
        let mut stack = VecDeque::new();
        stack.push_back(start_node);

        while let Some(node) = stack.pop_back() {
            if let Some(key) = node.value {
                result.push(key);
            }

            for child_node in node.children.values() {
                stack.push_back(child_node);
            }
        }
    }

    fn remove(&mut self, key_bytes: &[u8]) -> bool {
        self.root.remove(key_bytes, 0)
    }
}

impl Cache {
    fn new() -> Self {
        Cache {
            cached_values: BTreeMap::new(),
            pruned: BTreeSet::new(),
            cached_keys: CacheTrie::new(),
        }
    }

    /// Returns true if the pruned and cached values are both empty.
    pub fn is_empty(&self) -> bool {
        self.cached_values.is_empty() && self.pruned.is_empty()
    }

    fn insert_write(&mut self, key: Key, value: StoredValue) -> Result<(), bytesrepr::Error> {
        self.pruned.remove(&key);
        if self.cached_values.insert(key, (true, value)).is_none() {
            let key_bytes = key.to_bytes()?;
            self.cached_keys.insert(&key_bytes, key);
        };
        Ok(())
    }

    fn insert_read(&mut self, key: Key, value: StoredValue) -> Result<(), bytesrepr::Error> {
        let key_bytes = key.to_bytes()?;
        self.cached_keys.insert(&key_bytes, key);
        self.cached_values.entry(key).or_insert((false, value));
        Ok(())
    }

    fn prune(&mut self, key: Key) -> Result<(), bytesrepr::Error> {
        self.cached_values.remove(&key);
        self.cached_keys.remove(&key.to_bytes()?);
        self.pruned.insert(key);
        Ok(())
    }

    fn get(&self, key: &Key) -> Option<&StoredValue> {
        if self.pruned.contains(key) {
            return None;
        }
        self.cached_values.get(key).map(|(_dirty, value)| value)
    }

    /// Consumes self and returns only written values as values that were only read must be filtered
    /// out to prevent unnecessary writes.
    fn into_dirty_writes(self) -> (BTreeMap<Key, StoredValue>, BTreeSet<Key>) {
        let keys_to_prune = self.pruned;
        let stored_values: BTreeMap<Key, StoredValue> = self
            .cached_values
            .into_iter()
            .filter_map(|(key, (dirty, value))| if dirty { Some((key, value)) } else { None })
            .collect();
        debug!(
            "Cache::into_dirty_writes prune_count: {} store_count: {}",
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
    /// Max query depth
    pub max_query_depth: u64,
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

impl ScratchGlobalStateView {
    /// Returns true if the pruned and cached values are both empty.
    pub fn is_empty(&self) -> bool {
        self.cache.read().unwrap().is_empty()
    }
}

impl ScratchGlobalState {
    /// Creates a state from an existing environment, store, and root_hash.
    /// Intended to be used for testing.
    pub fn new(
        environment: Arc<LmdbEnvironment>,
        trie_store: Arc<LmdbTrieStore>,
        empty_root_hash: Digest,
        max_query_depth: u64,
    ) -> Self {
        ScratchGlobalState {
            cache: Arc::new(RwLock::new(Cache::new())),
            environment,
            trie_store,
            empty_root_hash,
            max_query_depth,
        }
    }

    /// Consume self and return inner cache.
    pub fn into_inner(self) -> (BTreeMap<Key, StoredValue>, BTreeSet<Key>) {
        let cache = mem::replace(&mut *self.cache.write().unwrap(), Cache::new());
        cache.into_dirty_writes()
    }
}

impl StateReader<Key, StoredValue> for ScratchGlobalStateView {
    type Error = GlobalStateError;

    fn read(&self, key: &Key) -> Result<Option<StoredValue>, Self::Error> {
        {
            let cache = self.cache.read().unwrap();
            if cache.pruned.contains(key) {
                return Ok(None);
            }
            if let Some(value) = cache.get(key) {
                return Ok(Some(value.clone()));
            }
        }
        let txn = self.environment.create_read_txn()?;
        let ret = match read::<Key, StoredValue, lmdb::RoTransaction, LmdbTrieStore, Self::Error>(
            &txn,
            self.trie_store.deref(),
            &self.root_hash,
            key,
        )? {
            ReadResult::Found(value) => {
                self.cache
                    .write()
                    .expect("poisoned scratch cache lock")
                    .insert_read(*key, value.clone())?;
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
        // if self.cache.is_empty() proceed else error
        if !self.is_empty() {
            return Err(Self::Error::CannotProvideProofsOverCachedData);
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
        let mut ret = Vec::new();
        let cache = self.cache.read().expect("poisoned scratch cache mutex");
        let cached_keys = cache.cached_keys.keys_with_prefix(prefix);
        ret.extend(cached_keys);

        let txn = self.environment.create_read_txn()?;
        let keys_iter = keys_with_prefix::<Key, StoredValue, _, _>(
            &txn,
            self.trie_store.deref(),
            &self.root_hash,
            prefix,
        );
        for result in keys_iter {
            match result {
                Ok(key) => {
                    // If the key is pruned then we won't return it. If the key is already cached,
                    // then it would have been picked up by the code above so we don't add it again
                    // to avoid duplicates.
                    if !cache.pruned.contains(&key) && !cache.cached_values.contains_key(&key) {
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
    fn commit(&self, state_hash: Digest, effects: Effects) -> Result<Digest, GlobalStateError> {
        let txn = self.environment.create_read_txn()?;
        for (key, kind) in effects.value().into_iter().map(TransformV2::destructure) {
            let cached_value = self.cache.read().unwrap().get(&key).cloned();
            let instruction = match (cached_value, kind) {
                (_, TransformKindV2::Identity) => {
                    // effectively a noop.
                    continue;
                }
                (None, TransformKindV2::Write(new_value)) => TransformInstruction::store(new_value),
                (None, transform_kind) => {
                    // It might be the case that for `Add*` operations we don't have the previous
                    // value in cache yet.
                    match read::<
                        Key,
                        StoredValue,
                        lmdb::RoTransaction,
                        LmdbTrieStore,
                        GlobalStateError,
                    >(&txn, self.trie_store.deref(), &state_hash, &key)?
                    {
                        ReadResult::Found(current_value) => {
                            match transform_kind.apply(current_value.clone()) {
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
                                ?transform_kind,
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
                (Some(current_value), transform_kind) => {
                    match transform_kind.apply(current_value) {
                        Ok(instruction) => instruction,
                        Err(err) => {
                            error!(?key, ?err, "Key found, but could not apply transform");
                            return Err(CommitError::TransformError(err).into());
                        }
                    }
                }
            };
            let mut cache = self.cache.write().unwrap();
            match instruction {
                TransformInstruction::Store(value) => {
                    cache.insert_write(key, value)?;
                }
                TransformInstruction::Prune(key) => {
                    cache.prune(key)?;
                }
            }
        }
        txn.commit()?;
        Ok(state_hash)
    }
}

impl StateProvider for ScratchGlobalState {
    type Reader = ScratchGlobalStateView;

    fn flush(&self, _: FlushRequest) -> FlushResult {
        if self.environment.is_manual_sync_enabled() {
            match self.environment.sync() {
                Ok(_) => FlushResult::Success,
                Err(err) => FlushResult::Failure(err.into()),
            }
        } else {
            FlushResult::ManualSyncDisabled
        }
    }

    fn empty_root(&self) -> Digest {
        self.empty_root_hash
    }

    fn tracking_copy(
        &self,
        hash: Digest,
    ) -> Result<Option<TrackingCopy<Self::Reader>>, GlobalStateError> {
        match self.checkout(hash)? {
            Some(tc) => Ok(Some(TrackingCopy::new(tc, self.max_query_depth))),
            None => Ok(None),
        }
    }

    fn checkout(&self, state_hash: Digest) -> Result<Option<Self::Reader>, GlobalStateError> {
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

    fn trie(&self, request: TrieRequest) -> TrieResult {
        let key = request.trie_key();
        let txn = match self.environment.create_read_txn() {
            Ok(ro) => ro,
            Err(err) => return TrieResult::Failure(err.into()),
        };
        let raw = match Store::<Digest, Trie<Digest, StoredValue>>::get_raw(
            &*self.trie_store,
            &txn,
            &key,
        ) {
            Ok(Some(bytes)) => TrieRaw::new(bytes),
            Ok(None) => {
                return TrieResult::ValueNotFound(key.to_string());
            }
            Err(err) => {
                return TrieResult::Failure(err);
            }
        };
        match txn.commit() {
            Ok(_) => match request.chunk_id() {
                Some(chunk_id) => TrieResult::Success {
                    element: TrieElement::Chunked(raw, chunk_id),
                },
                None => TrieResult::Success {
                    element: TrieElement::Raw(raw),
                },
            },
            Err(err) => TrieResult::Failure(err.into()),
        }
    }

    /// Persists a trie element.
    fn put_trie(&self, request: PutTrieRequest) -> PutTrieResult {
        // We only allow bottom-up persistence of trie elements.
        // Thus we do not persist the element unless we already have all of its descendants
        // persisted. It is safer to throw away the element and rely on a follow up attempt
        // to reacquire it later than to allow it to be persisted which would allow runtime
        // access to acquire a root hash that is missing one or more children which will
        // result in undefined behavior if a process attempts to access elements below that
        // root which are not held locally.
        let bytes = request.raw().inner();
        match self.missing_children(bytes) {
            Ok(missing_children) => {
                if !missing_children.is_empty() {
                    let hash = Digest::hash_into_chunks_if_necessary(bytes);
                    return PutTrieResult::Failure(GlobalStateError::MissingTrieNodeChildren(
                        hash,
                        request.take_raw(),
                        missing_children,
                    ));
                }
            }
            Err(err) => return PutTrieResult::Failure(err),
        };

        match self.environment.create_read_write_txn() {
            Ok(mut txn) => {
                match put_trie::<Key, StoredValue, RwTransaction, LmdbTrieStore, GlobalStateError>(
                    &mut txn,
                    &self.trie_store,
                    bytes,
                ) {
                    Ok(hash) => match txn.commit() {
                        Ok(_) => PutTrieResult::Success { hash },
                        Err(err) => PutTrieResult::Failure(err.into()),
                    },
                    Err(err) => PutTrieResult::Failure(err),
                }
            }
            Err(err) => PutTrieResult::Failure(err.into()),
        }
    }

    /// Finds all of the keys of missing directly descendant `Trie<K,V>` values
    fn missing_children(&self, trie_raw: &[u8]) -> Result<Vec<Digest>, GlobalStateError> {
        let txn = self.environment.create_read_txn()?;
        let missing_descendants = missing_children::<
            Key,
            StoredValue,
            lmdb::RoTransaction,
            LmdbTrieStore,
            GlobalStateError,
        >(&txn, self.trie_store.deref(), trie_raw)?;
        txn.commit()?;
        Ok(missing_descendants)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use lmdb::DatabaseFlags;
    use tempfile::tempdir;

    use casper_types::{
        account::AccountHash,
        execution::{Effects, TransformKindV2, TransformV2},
        CLValue, Digest,
    };

    use super::*;
    use crate::global_state::{
        state::{lmdb::LmdbGlobalState, CommitProvider},
        trie_store::operations::{write, WriteResult},
    };

    #[cfg(test)]
    use crate::global_state::{DEFAULT_MAX_DB_SIZE, DEFAULT_MAX_READERS};

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

    pub(crate) fn create_test_transforms() -> Effects {
        let mut effects = Effects::new();
        let transform = TransformV2::new(
            Key::Account(AccountHash::new([3u8; 32])),
            TransformKindV2::Write(StoredValue::CLValue(CLValue::from_t("one").unwrap())),
        );
        effects.push(transform);
        effects
    }

    pub(crate) struct TestState {
        state: LmdbGlobalState,
        root_hash: Digest,
    }

    #[cfg(test)]
    pub(crate) fn create_test_state() -> TestState {
        let temp_dir = tempdir().unwrap();
        let environment = Arc::new(
            LmdbEnvironment::new(
                temp_dir.path(),
                DEFAULT_MAX_DB_SIZE,
                DEFAULT_MAX_READERS,
                true,
            )
            .unwrap(),
        );
        let trie_store =
            Arc::new(LmdbTrieStore::new(&environment, None, DatabaseFlags::empty()).unwrap());

        let state = LmdbGlobalState::empty(
            environment,
            trie_store,
            crate::global_state::DEFAULT_MAX_QUERY_DEPTH,
        )
        .unwrap();
        let mut current_root = state.empty_root_hash;
        {
            let mut txn = state.environment.create_read_write_txn().unwrap();

            for TestPair { key, value } in &create_test_pairs() {
                match write::<_, _, _, LmdbTrieStore, GlobalStateError>(
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

        let effects = {
            let mut tmp = Effects::new();
            for TestPair { key, value } in &test_pairs_updated {
                let transform = TransformV2::new(*key, TransformKindV2::Write(value.to_owned()));
                tmp.push(transform);
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

        let effects = {
            let mut tmp = Effects::new();
            for TestPair { key, value } in &test_pairs_updated {
                let transform = TransformV2::new(*key, TransformKindV2::Write(value.to_owned()));
                tmp.push(transform);
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

        let effects = {
            let mut tmp = Effects::new();
            for TestPair { key, value } in &test_pairs_updated {
                let transform = TransformV2::new(*key, TransformKindV2::Write(value.to_owned()));
                tmp.push(transform);
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

    #[test]
    fn cache_trie_basic_insert_get() {
        let mut trie = CacheTrie::new();
        let key_hello = Key::Hash(*b"hello...........................");
        let key_world = Key::Hash(*b"world...........................");
        let key_hey = Key::Hash(*b"hey.............................");

        trie.insert(b"hello", key_hello);
        trie.insert(b"world", key_world);
        trie.insert(b"hey", key_hey);

        assert_eq!(trie.keys_with_prefix(b"he"), vec![key_hello, key_hey]);
        assert_eq!(trie.keys_with_prefix(b"wo"), vec![key_world]);
    }

    #[test]
    fn cache_trie_overlapping_prefix() {
        let mut trie = CacheTrie::new();
        let key_apple = Key::Hash(*b"apple...........................");
        let key_app = Key::Hash(*b"app.............................");
        let key_apron = Key::Hash(*b"apron...........................");

        trie.insert(b"apple", key_apple);
        trie.insert(b"app", key_app);
        trie.insert(b"apron", key_apron);

        assert_eq!(
            trie.keys_with_prefix(b"ap"),
            vec![key_app, key_apple, key_apron]
        );
        assert_eq!(trie.keys_with_prefix(b"app"), vec![key_app, key_apple]);
    }

    #[test]
    fn cache_trie_leaf_removal() {
        let mut trie = CacheTrie::new();
        let key_cat = Key::Hash(*b"cat.............................");
        let key_category = Key::Hash(*b"category........................");

        trie.insert(b"cat", key_cat);
        trie.insert(b"category", key_category);

        trie.remove(b"category");
        assert_eq!(trie.keys_with_prefix(b"ca"), vec![key_cat]);
    }

    #[test]
    fn cache_trie_internal_node_removal() {
        let mut trie = CacheTrie::new();
        let key_be = Key::Hash(*b"be..............................");
        let key_berry = Key::Hash(*b"berry...........................");

        trie.insert(b"be", key_be);
        trie.insert(b"berry", key_berry);

        trie.remove(b"be");
        assert_eq!(trie.keys_with_prefix(b"be"), vec![key_berry]);
    }

    #[test]
    fn cache_trie_non_existent_prefix() {
        let mut trie = CacheTrie::new();

        let key_apple = Key::Hash(*b"apple...........................");
        let key_mango = Key::Hash(*b"mango...........................");

        trie.insert(b"apple", key_apple);
        trie.insert(b"mango", key_mango);

        assert_eq!(trie.keys_with_prefix(b"b"), Vec::<Key>::new());
    }

    #[test]
    fn cache_trie_empty_trie_search() {
        let trie = CacheTrie::<Key>::new();

        assert_eq!(trie.keys_with_prefix(b""), Vec::<Key>::new());
    }
}
