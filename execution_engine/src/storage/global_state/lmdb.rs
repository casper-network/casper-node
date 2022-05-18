use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::{Arc, RwLock},
};

use casper_hashing::{ChunkWithProof, Digest};
use casper_types::{bytesrepr::Bytes, Key, StoredValue};
use tracing::info;

use crate::{
    shared::{additive_map::AdditiveMap, newtypes::CorrelationId, transform::Transform},
    storage::{
        error,
        global_state::{
            commit, put_stored_values, scratch::ScratchGlobalState, CommitProvider, StateProvider,
            StateReader,
        },
        store::Store,
        transaction_source::{lmdb::LmdbEnvironment, Transaction, TransactionSource},
        trie::{
            merkle_proof::TrieMerkleProof, operations::create_hashed_empty_trie, Trie, TrieOrChunk,
            TrieOrChunkId,
        },
        trie_store::{
            lmdb::{LmdbTrieStore, ScratchTrieStore},
            operations::{
                descendant_trie_keys, keys_with_prefix, missing_trie_keys, put_trie, read,
                read_with_proof, ReadResult,
            },
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
    digests_without_missing_descendants: RwLock<HashSet<Digest>>,
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
            environment.env().sync(true)?;
            root_hash
        };
        Ok(LmdbGlobalState::new(environment, trie_store, root_hash))
    }

    /// Creates a state from an existing environment, store, and root_hash.
    /// Intended to be used for testing.
    pub fn new(
        environment: Arc<LmdbEnvironment>,
        trie_store: Arc<LmdbTrieStore>,
        empty_root_hash: Digest,
    ) -> Self {
        LmdbGlobalState {
            environment,
            trie_store,
            empty_root_hash,
            digests_without_missing_descendants: Default::default(),
        }
    }

    /// Creates an in-memory cache for changes written.
    pub fn create_scratch(&self) -> ScratchGlobalState {
        ScratchGlobalState::new(
            Arc::clone(&self.environment),
            Arc::clone(&self.trie_store),
            self.empty_root_hash,
        )
    }

    /// Write stored values to LMDB.
    pub fn put_stored_values(
        &self,
        correlation_id: CorrelationId,
        prestate_hash: Digest,
        stored_values: HashMap<Key, StoredValue>,
    ) -> Result<Digest, error::Error> {
        let scratch_trie = self.get_scratch_store();
        let new_state_root = put_stored_values::<_, _, error::Error>(
            &scratch_trie,
            &scratch_trie,
            correlation_id,
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

impl CommitProvider for LmdbGlobalState {
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

    fn empty_root(&self) -> Digest {
        self.empty_root_hash
    }

    fn get_trie(
        &self,
        _correlation_id: CorrelationId,
        trie_or_chunk_id: TrieOrChunkId,
    ) -> Result<Option<TrieOrChunk>, Self::Error> {
        let TrieOrChunkId(trie_index, trie_key) = trie_or_chunk_id;
        let txn = self.environment.create_read_txn()?;
        let bytes = Store::<Digest, Trie<Digest, StoredValue>>::get_raw(
            &*self.trie_store,
            &txn,
            &trie_key,
        )?;

        let maybe_trie_or_chunk = bytes.map_or_else(
            || Ok(None),
            |bytes| {
                if bytes.len() <= ChunkWithProof::CHUNK_SIZE_BYTES {
                    Ok(Some(TrieOrChunk::Trie(bytes)))
                } else {
                    let chunk_with_proof = ChunkWithProof::new(&bytes, trie_index)?;
                    Ok(Some(TrieOrChunk::ChunkWithProof(chunk_with_proof)))
                }
            },
        );

        txn.commit()?;
        maybe_trie_or_chunk
    }

    fn get_trie_full(
        &self,
        _correlation_id: CorrelationId,
        trie_key: &Digest,
    ) -> Result<Option<Bytes>, Self::Error> {
        let txn = self.environment.create_read_txn()?;
        let ret: Option<Bytes> =
            Store::<Digest, Trie<Digest, StoredValue>>::get_raw(&*self.trie_store, &txn, trie_key)?;
        txn.commit()?;
        Ok(ret)
    }

    fn put_trie(&self, correlation_id: CorrelationId, trie: &[u8]) -> Result<Digest, Self::Error> {
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

    /// Finds all of the keys of missing descendant `Trie<K,V>` values.
    fn missing_trie_keys(
        &self,
        correlation_id: CorrelationId,
        trie_keys: Vec<Digest>,
    ) -> Result<Vec<Digest>, Self::Error> {
        let trie_count = {
            let digests_without_missing_descendants = self
                .digests_without_missing_descendants
                .read()
                .expect("digest cache read lock");
            trie_keys
                .iter()
                .filter(|digest| !digests_without_missing_descendants.contains(digest))
                .count()
        };
        if trie_count == 0 {
            info!("no need to call missing_trie_keys");
            Ok(vec![])
        } else {
            let txn = self.environment.create_read_txn()?;
            let missing_descendants = missing_trie_keys::<
                Key,
                StoredValue,
                lmdb::RoTransaction,
                LmdbTrieStore,
                Self::Error,
            >(
                correlation_id,
                &txn,
                self.trie_store.deref(),
                trie_keys.clone(),
                &self
                    .digests_without_missing_descendants
                    .read()
                    .expect("digest cache read lock"),
            )?;
            if missing_descendants.is_empty() {
                // There were no missing descendants on `trie_keys`, let's add them *and all of
                // their descendants* to the cache.

                let mut all_descendants: HashSet<Digest> = HashSet::new();
                all_descendants.extend(&trie_keys);
                all_descendants.extend(descendant_trie_keys::<
                    Key,
                    StoredValue,
                    lmdb::RoTransaction,
                    LmdbTrieStore,
                    Self::Error,
                >(
                    &txn,
                    self.trie_store.deref(),
                    trie_keys,
                    &self
                        .digests_without_missing_descendants
                        .read()
                        .expect("digest cache read lock"),
                )?);

                self.digests_without_missing_descendants
                    .write()
                    .expect("digest cache write lock")
                    .extend(all_descendants.into_iter());
            }
            txn.commit()?;

            Ok(missing_descendants)
        }
    }
}

#[cfg(test)]
mod tests {
    use lmdb::DatabaseFlags;
    use tempfile::tempdir;

    use casper_hashing::Digest;
    use casper_types::{account::AccountHash, bytesrepr, CLValue};

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

    // Creates the test pairs that contain data of size
    // greater than the chunk limit.
    fn create_test_pairs_with_large_data() -> [TestPair; 2] {
        let val = CLValue::from_t(
            String::from_utf8(vec![b'a'; ChunkWithProof::CHUNK_SIZE_BYTES * 2]).unwrap(),
        )
        .unwrap();
        [
            TestPair {
                key: Key::Account(AccountHash::new([1_u8; 32])),
                value: StoredValue::CLValue(val.clone()),
            },
            TestPair {
                key: Key::Account(AccountHash::new([2_u8; 32])),
                value: StoredValue::CLValue(val),
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

    fn create_test_state(pairs_creator: fn() -> [TestPair; 2]) -> (LmdbGlobalState, Digest) {
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

        let ret = LmdbGlobalState::empty(environment, trie_store).unwrap();
        let mut current_root = ret.empty_root_hash;
        {
            let mut txn = ret.environment.create_read_write_txn().unwrap();

            for TestPair { key, value } in &(pairs_creator)() {
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
        let (state, root_hash) = create_test_state(create_test_pairs);
        let checkout = state.checkout(root_hash).unwrap().unwrap();
        for TestPair { key, value } in create_test_pairs().iter().cloned() {
            assert_eq!(Some(value), checkout.read(correlation_id, &key).unwrap());
        }
    }

    #[test]
    fn checkout_fails_if_unknown_hash_is_given() {
        let (state, _) = create_test_state(create_test_pairs);
        let fake_hash: Digest = Digest::hash(&[1u8; 32]);
        let result = state.checkout(fake_hash).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn commit_updates_state() {
        let correlation_id = CorrelationId::new();
        let test_pairs_updated = create_test_pairs_updated();

        let (state, root_hash) = create_test_state(create_test_pairs);

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

        let (state, root_hash) = create_test_state(create_test_pairs);

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

    #[test]
    fn returns_trie_or_chunk() {
        let correlation_id = CorrelationId::new();
        let (state, root_hash) = create_test_state(create_test_pairs_with_large_data);

        // Expect `Trie` with NodePointer when asking with a root hash.
        let trie = state
            .get_trie(correlation_id, TrieOrChunkId(0, root_hash))
            .expect("should get trie correctly")
            .expect("should be Some()");
        assert!(matches!(trie, TrieOrChunk::Trie(_)));

        // Expect another `Trie` with two LeafPointers.
        let trie = state
            .get_trie(
                correlation_id,
                TrieOrChunkId(0, extract_next_hash_from_trie(trie)),
            )
            .expect("should get trie correctly")
            .expect("should be Some()");
        assert!(matches!(trie, TrieOrChunk::Trie(_)));

        // Now, the next hash will point to the actual leaf, which as we expect
        // contains large data, so we expect to get `ChunkWithProof`.
        let hash = extract_next_hash_from_trie(trie);
        let chunk = match state
            .get_trie(correlation_id, TrieOrChunkId(0, hash))
            .expect("should get trie correctly")
            .expect("should be Some()")
        {
            TrieOrChunk::ChunkWithProof(chunk) => chunk,
            other => panic!("expected ChunkWithProof, got {:?}", other),
        };

        assert_eq!(chunk.proof().root_hash(), hash);

        // try to read all the chunks
        let count = chunk.proof().count();
        let mut chunks = vec![chunk];
        for i in 1..count {
            let chunk = match state
                .get_trie(correlation_id, TrieOrChunkId(i, hash))
                .expect("should get trie correctly")
                .expect("should be Some()")
            {
                TrieOrChunk::ChunkWithProof(chunk) => chunk,
                other => panic!("expected ChunkWithProof, got {:?}", other),
            };
            chunks.push(chunk);
        }

        // there should be no chunk with index `count`
        assert!(matches!(
            state.get_trie(correlation_id, TrieOrChunkId(count, hash)),
            Err(error::Error::MerkleConstruction(_))
        ));

        // all chunks should be valid
        assert!(chunks.iter().all(|chunk| chunk.verify().is_ok()));

        let data: Vec<u8> = chunks
            .into_iter()
            .flat_map(|chunk| chunk.into_chunk())
            .collect();

        let trie: Trie<Key, StoredValue> =
            bytesrepr::deserialize(data).expect("trie should deserialize correctly");

        // should be deserialized to a leaf
        assert!(matches!(trie, Trie::Leaf { .. }));
    }

    fn extract_next_hash_from_trie(trie_or_chunk: TrieOrChunk) -> Digest {
        let next_hash = if let TrieOrChunk::Trie(trie_bytes) = trie_or_chunk {
            if let Trie::Node { pointer_block } =
                bytesrepr::deserialize::<Trie<Key, StoredValue>>(Vec::<u8>::from(trie_bytes))
                    .expect("Could not parse trie bytes")
            {
                if pointer_block.child_count() == 0 {
                    panic!("expected children");
                }
                let (_, ptr) = pointer_block.as_indexed_pointers().next().unwrap();
                match ptr {
                    crate::storage::trie::Pointer::LeafPointer(ptr)
                    | crate::storage::trie::Pointer::NodePointer(ptr) => ptr,
                }
            } else {
                panic!("expected `Node`");
            }
        } else {
            panic!("expected `Trie`");
        };
        next_hash
    }
}
