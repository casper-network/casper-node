use lmdb::{DatabaseFlags, Transaction};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, RwLock},
    thread,
    time::{Duration, Instant},
};
use tracing::{info, warn};

use casper_hashing::{ChunkWithProof, Digest};
use casper_types::{
    bytesrepr::{self, Bytes, ToBytes},
    Key, StoredValue,
};

use crate::{
    shared::{additive_map::AdditiveMap, newtypes::CorrelationId, transform::Transform},
    storage::{
        error::{self, db::DbError},
        global_state::{
            commit_effects, put_stored_values, scratch::ScratchGlobalState, CommitProvider,
            StateProvider, StateReader,
        },
        store::{Readable, Store, Writable},
        trie::{
            merkle_proof::TrieMerkleProof, operations::create_hashed_empty_trie, Pointer, Trie,
            TrieOrChunk, TrieOrChunkId,
        },
        trie_store::{
            db::{RocksDbTrieStore, ScratchTrieStore},
            operations::{
                descendant_trie_keys, keys_with_prefix, missing_trie_keys, put_trie_bytes, read,
                read_with_proof, ReadResult,
            },
        },
    },
};

// This module is intended to be private and will be sunset after migration to rocksdb is complete.
mod legacy_lmdb {

    use std::path::Path;

    use lmdb::{self, Environment, EnvironmentFlags};

    use crate::storage::{error, MAX_DBS};

    /// Filename for the LMDB database created by the EE.
    const EE_LMDB_FILENAME: &str = "data.lmdb";

    // LMDB database name.
    pub(super) const LMDB_DATABASE_NAME: &str = "TRIE_STORE";

    /// The environment for an LMDB-backed trie store.
    ///
    /// Wraps [`lmdb::Environment`].
    #[derive(Debug)]
    pub(crate) struct LmdbEnvironment {
        env: Environment,
    }

    impl LmdbEnvironment {
        /// Constructor for `LmdbEnvironment`.
        pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, error::Error> {
            // Set the flag to manage our own directory like in the storage component.
            let lmdb_flags = EnvironmentFlags::NO_SUB_DIR | EnvironmentFlags::NO_READAHEAD;
            let env = Environment::new()
                .set_flags(lmdb_flags)
                .set_max_dbs(MAX_DBS)
                .set_map_size(0)
                .set_max_readers(1)
                .open(&path.as_ref().join(EE_LMDB_FILENAME))?;
            Ok(LmdbEnvironment { env })
        }

        /// Returns a reference to the wrapped `Environment`.
        pub fn env(&self) -> &Environment {
            &self.env
        }
    }
}

/// Global state implemented against a database as a backing data store.
#[derive(Clone)]
pub struct DbGlobalState {
    /// Optional path to legacy LMDB files - loaded for migration only if present.
    maybe_lmdb_path: Option<PathBuf>,
    /// Empty root hash used for a new trie.
    pub(crate) empty_root_hash: Digest,
    /// Handle to rocksdb.
    pub(crate) trie_store: RocksDbTrieStore,
    digests_without_missing_descendants: Arc<RwLock<HashSet<Digest>>>,
}

/// Represents a "view" of global state at a particular root hash.
#[derive(Clone)]
pub struct DbGlobalStateView {
    /// Root hash of this "view".
    root_hash: Digest,
    /// Handle to rocksdb.
    trie_store: RocksDbTrieStore,
}

impl DbGlobalState {
    /// Return the rocksdb trie store.
    pub fn get_trie_store(&self) -> &RocksDbTrieStore {
        &self.trie_store
    }

    /// Migrate data at the given state roots (if they exist) from lmdb to rocksdb.
    /// This function uses std::thread::sleep, is otherwise intensive and so needs to be called with
    /// tokio::task::spawn_blocking. `force=true` will cause us to always traverse recursively when
    /// searching for missing descendants.
    pub(crate) fn migrate_state_root_to_rocksdb(
        &self,
        state_root: Digest,
        limit_rate: bool,
        force: bool,
    ) -> Result<(), error::Error> {
        let (lmdb_environment, lmdb_db) = match self.maybe_lmdb_path.as_ref() {
            Some(path) => {
                let env = legacy_lmdb::LmdbEnvironment::new(path)?;
                let db = env.env().create_db(
                    Some(legacy_lmdb::LMDB_DATABASE_NAME),
                    DatabaseFlags::empty(),
                )?;
                (env, db)
            }
            None => {
                info!("No existing lmdb database found, skipping migration.");
                return Ok(());
            }
        };

        let mut missing_trie_keys = vec![state_root];
        let start_time = Instant::now();
        let mut interval_start = start_time;
        let mut heartbeat_interval = Instant::now();

        const BYTES_PER_SEC: u64 = 8 * 1024 * 1024;
        const INTERVAL_MILLIS: u64 = 10;
        const TARGET_INTERVAL_BYTES: u64 = (BYTES_PER_SEC / 1000) * INTERVAL_MILLIS;
        const INTERVAL_DURATION: Duration = Duration::from_millis(INTERVAL_MILLIS);

        let mut interval_bytes = 0;
        let mut total_tries: u64 = 0;
        let mut total_bytes: u64 = 0;

        let mut time_searching_for_trie_keys = Duration::from_secs(0);

        while let Some(next_trie_key) = missing_trie_keys.pop() {
            if limit_rate {
                let elapsed = interval_start.elapsed();
                if interval_bytes >= TARGET_INTERVAL_BYTES {
                    if elapsed < INTERVAL_DURATION {
                        thread::sleep(INTERVAL_DURATION - elapsed);
                    }
                    interval_start = Instant::now();
                    interval_bytes = 0;
                }
            }

            // For user feedback, update on progress if this takes longer than 10 seconds.
            if heartbeat_interval.elapsed().as_secs() > 10 {
                info!(
                    "trie migration progress: bytes copied {}, tries copied {}",
                    total_bytes, total_tries,
                );
                heartbeat_interval = Instant::now();
            }

            let lmdb_txn = lmdb_environment.env().begin_ro_txn()?;

            let trie_key_bytes = next_trie_key.to_bytes()?;
            match lmdb_txn.get(lmdb_db, &trie_key_bytes) {
                Ok(value_bytes) => {
                    let key_bytes = next_trie_key.to_bytes()?;
                    let read_bytes = key_bytes.len() as u64 + value_bytes.len() as u64;
                    interval_bytes += read_bytes;
                    total_bytes += read_bytes;
                    total_tries += 1;

                    self.trie_store.write(&key_bytes, value_bytes)?;

                    memoized_find_missing_descendants(
                        Bytes::from(value_bytes),
                        &self.trie_store,
                        &mut missing_trie_keys,
                        &mut time_searching_for_trie_keys,
                        force,
                    )?;
                }
                Err(lmdb::Error::NotFound) => {
                    // Gracefully handle roots that only exist in rocksdb, unless we're forcing a
                    // refresh of children (in the case where we have an incomplete root).
                    if force || self.trie_store.read(&trie_key_bytes)?.is_none() {
                        return Err(error::Error::CorruptLmdbStateRootDuringMigrationToRocksDb {
                            trie_key: next_trie_key,
                            state_root,
                        });
                    }
                }
                Err(other) => return Err(error::Error::Db(DbError::Lmdb(other))),
            }
            lmdb_txn.commit()?;
        }

        info!(
            %total_bytes,
            %total_tries,
            time_migration_took_micros = %start_time.elapsed().as_micros(),
            time_searching_for_trie_keys_micros = %time_searching_for_trie_keys.as_micros(),
            "trie migration complete",
        );

        Ok(())
    }

    /// Creates an empty state from an existing environment and trie_store.
    pub fn empty(
        maybe_lmdb_path: Option<PathBuf>,
        rocksdb_store: RocksDbTrieStore,
    ) -> Result<Self, error::Error> {
        let empty_root_hash: Digest = {
            let (root_hash, root) = create_hashed_empty_trie::<Key, StoredValue>()?;
            rocksdb_store.put(&root_hash, &root)?;
            root_hash
        };
        Ok(DbGlobalState {
            maybe_lmdb_path,
            empty_root_hash,
            trie_store: rocksdb_store,
            digests_without_missing_descendants: Default::default(),
        })
    }

    /// Write stored values to RocksDb.
    pub fn put_stored_values(
        &self,
        correlation_id: CorrelationId,
        prestate_hash: Digest,
        stored_values: HashMap<Key, StoredValue>,
    ) -> Result<Digest, error::Error> {
        let scratch_trie = self.get_scratch_store();
        let new_state_root = put_stored_values::<_, error::Error>(
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
        ScratchTrieStore::new(self.trie_store.clone())
    }

    /// Creates an in-memory cache for changes written.
    pub fn create_scratch(&self) -> ScratchGlobalState {
        ScratchGlobalState::new(self.clone())
    }
}

fn memoized_find_missing_descendants(
    value_bytes: Bytes,
    trie_store: &RocksDbTrieStore,
    missing_trie_keys: &mut Vec<Digest>,
    time_in_missing_trie_keys: &mut Duration,
    force: bool,
) -> Result<(), error::Error> {
    // A first bytes of `0` indicates a leaf. We short-circuit the function here to speed things up.
    if let Some(0u8) = value_bytes.get(0) {
        return Ok(());
    }
    let start_trie_keys = Instant::now();
    let trie: Trie<Key, StoredValue> = bytesrepr::deserialize(value_bytes.into())?;
    match trie {
        Trie::Leaf { .. } => {
            // If `bytesrepr` is functioning correctly, this should never be reached (see
            // optimization above), but it is still correct do nothing here.
            warn!("did not expect to see a trie leaf in `find_missing_descendents` after shortcut");
        }
        Trie::Node { pointer_block } => {
            for (_index, ptr) in pointer_block.as_indexed_pointers() {
                find_missing_trie_keys(ptr, force, missing_trie_keys, trie_store)?;
            }
        }
        Trie::Extension { affix: _, pointer } => {
            find_missing_trie_keys(pointer, force, missing_trie_keys, trie_store)?;
        }
    }
    *time_in_missing_trie_keys += start_trie_keys.elapsed();
    Ok(())
}

fn find_missing_trie_keys(
    ptr: Pointer,
    force: bool,
    missing_trie_keys: &mut Vec<Digest>,
    rocksdb: &RocksDbTrieStore,
) -> Result<(), error::Error> {
    let ptr = match ptr {
        Pointer::LeafPointer(pointer) | Pointer::NodePointer(pointer) => pointer,
    };
    if force {
        missing_trie_keys.push(ptr);
    } else {
        let existing = rocksdb.read(&ptr.to_bytes()?)?;
        if existing.is_none() {
            missing_trie_keys.push(ptr);
        }
    }
    Ok(())
}

impl StateReader<Key, StoredValue> for DbGlobalStateView {
    type Error = error::Error;

    fn read(
        &self,
        correlation_id: CorrelationId,
        key: &Key,
    ) -> Result<Option<StoredValue>, Self::Error> {
        let ret = match read::<_, _, _, Self::Error>(
            correlation_id,
            &self.trie_store,
            &self.root_hash,
            key,
        )? {
            ReadResult::Found(value) => Some(value),
            ReadResult::NotFound => None,
            ReadResult::RootNotFound => panic!("DbGlobalState has invalid root"),
        };
        Ok(ret)
    }

    fn read_with_proof(
        &self,
        correlation_id: CorrelationId,
        key: &Key,
    ) -> Result<Option<TrieMerkleProof<Key, StoredValue>>, Self::Error> {
        let ret = match read_with_proof::<_, _, _, Self::Error>(
            correlation_id,
            &self.trie_store,
            &self.root_hash,
            key,
        )? {
            ReadResult::Found(value) => Some(value),
            ReadResult::NotFound => None,
            ReadResult::RootNotFound => panic!("DbGlobalState has invalid root"),
        };
        Ok(ret)
    }

    fn keys_with_prefix(
        &self,
        correlation_id: CorrelationId,
        prefix: &[u8],
    ) -> Result<Vec<Key>, Self::Error> {
        let keys_iter = keys_with_prefix::<Key, StoredValue, RocksDbTrieStore>(
            correlation_id,
            &self.trie_store,
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
        Ok(ret)
    }
}

impl CommitProvider for DbGlobalState {
    fn commit_effects(
        &self,
        correlation_id: CorrelationId,
        prestate_hash: Digest,
        effects: AdditiveMap<Key, Transform>,
    ) -> Result<Digest, Self::Error> {
        commit_effects::<RocksDbTrieStore, _, Self::Error>(
            &self.trie_store,
            correlation_id,
            prestate_hash,
            effects,
        )
        .map_err(Into::into)
    }
}

impl StateProvider for DbGlobalState {
    type Error = error::Error;

    type Reader = DbGlobalStateView;

    fn checkout(&self, state_hash: Digest) -> Result<Option<Self::Reader>, Self::Error> {
        let rocksdb_store = self.trie_store.clone();
        let maybe_root: Option<Trie<Key, StoredValue>> = rocksdb_store.get(&state_hash)?;
        let maybe_state = maybe_root.map(|_| DbGlobalStateView {
            root_hash: state_hash,
            trie_store: rocksdb_store,
        });
        Ok(maybe_state)
    }

    fn empty_root(&self) -> Digest {
        self.empty_root_hash
    }

    fn get_trie_or_chunk(
        &self,
        _correlation_id: CorrelationId,
        trie_or_chunk_id: TrieOrChunkId,
    ) -> Result<Option<TrieOrChunk>, Self::Error> {
        let TrieOrChunkId(trie_index, trie_key) = trie_or_chunk_id;

        let bytes =
            Store::<Digest, Trie<Digest, StoredValue>>::get_raw(&self.trie_store, &trie_key)?;

        bytes.map_or_else(
            || Ok(None),
            |bytes| {
                if bytes.len() <= ChunkWithProof::CHUNK_SIZE_BYTES {
                    Ok(Some(TrieOrChunk::Trie(bytes)))
                } else {
                    let chunk_with_proof = ChunkWithProof::new(&bytes, trie_index)?;
                    Ok(Some(TrieOrChunk::ChunkWithProof(chunk_with_proof)))
                }
            },
        )
    }

    fn get_trie_full(
        &self,
        _correlation_id: CorrelationId,
        trie_key: &Digest,
    ) -> Result<Option<Bytes>, Self::Error> {
        Store::<Digest, Trie<Digest, StoredValue>>::get_raw(&self.trie_store, trie_key)
    }

    fn put_trie_bytes(
        &self,
        correlation_id: CorrelationId,
        trie_bytes: &[u8],
    ) -> Result<Digest, Self::Error> {
        let trie_store = self.trie_store.clone();
        let trie_hash = put_trie_bytes::<Key, StoredValue, _, Self::Error>(
            correlation_id,
            &trie_store,
            trie_bytes,
        )?;
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
            let missing_descendants = missing_trie_keys::<Key, StoredValue, _, Self::Error>(
                correlation_id,
                &self.trie_store,
                trie_keys.clone(),
                &self
                    .digests_without_missing_descendants
                    .read()
                    .expect("digest cache read lock"),
            )?;
            if missing_descendants.is_empty() {
                // There were no missing descendants on `trie_keys`, let's add them *and all of
                // their descendants* to the cache.

                let mut all_descendants = HashSet::new();
                all_descendants.extend(&trie_keys);
                all_descendants.extend(descendant_trie_keys::<Key, StoredValue, _, Self::Error>(
                    &self.trie_store,
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

            Ok(missing_descendants)
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use casper_hashing::Digest;
    use casper_types::{account::AccountHash, CLValue};

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

    fn create_test_state(pairs_creator: fn() -> [TestPair; 2]) -> (DbGlobalState, Digest) {
        let correlation_id = CorrelationId::new();
        let rocksdb_temp_dir = tempdir().unwrap();

        let trie_store = RocksDbTrieStore::new(&rocksdb_temp_dir.path()).unwrap();
        let engine_state = DbGlobalState::empty(None, trie_store).unwrap();
        let mut current_root = engine_state.empty_root_hash;
        for TestPair { key, value } in pairs_creator() {
            let mut stored_values = HashMap::new();
            stored_values.insert(key, value);
            current_root = engine_state
                .put_stored_values(correlation_id, current_root, stored_values)
                .unwrap();
        }
        (engine_state, current_root)
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

        let updated_hash = state
            .commit_effects(correlation_id, root_hash, effects)
            .unwrap();

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

        let updated_hash = state
            .commit_effects(correlation_id, root_hash, effects)
            .unwrap();

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
            .get_trie_or_chunk(correlation_id, TrieOrChunkId(0, root_hash))
            .expect("should get trie correctly")
            .expect("should be Some()");
        assert!(matches!(trie, TrieOrChunk::Trie(_)));

        // Expect another `Trie` with two LeafPointers.
        let trie = state
            .get_trie_or_chunk(
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
            .get_trie_or_chunk(correlation_id, TrieOrChunkId(0, hash))
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
                .get_trie_or_chunk(correlation_id, TrieOrChunkId(i, hash))
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
            state.get_trie_or_chunk(correlation_id, TrieOrChunkId(count, hash)),
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
