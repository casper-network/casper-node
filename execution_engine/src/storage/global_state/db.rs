use std::{
    collections::HashMap,
    path::Path,
    sync::Arc,
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
        error,
        global_state::{
            commit, put_stored_values, scratch::ScratchGlobalState, CommitProvider, StateProvider,
            StateReader,
        },
        store::Store,
        transaction_source::{
            self,
            db::{LmdbEnvironment, RocksDb, RocksDbStore},
            Readable, Transaction, TransactionSource, Writable,
        },
        trie::{
            merkle_proof::TrieMerkleProof, operations::create_hashed_empty_trie, Pointer, Trie,
            TrieOrChunk, TrieOrChunkId,
        },
        trie_store::{
            db::LmdbTrieStore,
            operations::{
                keys_with_prefix, missing_trie_keys, put_trie, read, read_with_proof, ReadResult,
            },
        },
    },
};

/// Global state implemented against a database as a backing data store.
#[derive(Clone)]
pub struct DbGlobalState {
    /// Environment for LMDB.
    pub(crate) lmdb_environment: Arc<LmdbEnvironment>,
    /// Trie store held within LMDB.
    pub(crate) lmdb_trie_store: Arc<LmdbTrieStore>,
    /// Empty root hash used for a new trie.
    pub(crate) empty_root_hash: Digest,
    /// Handle to rocksdb.
    pub(crate) rocksdb_store: RocksDbStore,
}

/// Represents a "view" of global state at a particular root hash.
#[derive(Clone)]
pub struct DbGlobalStateView {
    /// Root hash of this "view".
    root_hash: Digest,
    /// Handle to rocksdb.
    rocksdb_store: RocksDbStore,
}

impl DbGlobalState {
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

            let lmdb_txn = self.lmdb_environment.create_read_txn()?;

            // TODO(dwerner): after this migration clean up read and write traits to fix:
            let mut handle = self.rocksdb_store.rocksdb.clone();
            let rocksdb = self.rocksdb_store.rocksdb.clone();

            match lmdb_txn.read(self.lmdb_trie_store.db, &next_trie_key.to_bytes()?)? {
                Some(value_bytes) => {
                    let key_bytes = next_trie_key.to_bytes()?;
                    let read_bytes = key_bytes.len() as u64 + value_bytes.len() as u64;
                    interval_bytes += read_bytes;
                    total_bytes += read_bytes;
                    total_tries += 1;

                    handle.write(rocksdb.clone(), &key_bytes, &value_bytes)?;

                    memoized_find_missing_descendants(
                        value_bytes,
                        handle,
                        rocksdb,
                        &mut missing_trie_keys,
                        &mut time_searching_for_trie_keys,
                        force,
                    )?;
                }
                None => {
                    return Err(error::Error::CorruptLmdbStateRootDuringMigrationToRocksDb {
                        trie_key: next_trie_key,
                        state_root,
                    });
                }
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
        environment: Arc<LmdbEnvironment>,
        trie_store: Arc<LmdbTrieStore>,
        rocksdb_path: impl AsRef<Path>,
    ) -> Result<Self, error::Error> {
        let rocksdb_store =
            RocksDbStore::new(rocksdb_path, transaction_source::rocksdb_defaults())?;

        let empty_root_hash: Digest = {
            let (root_hash, root) = create_hashed_empty_trie::<Key, StoredValue>()?;
            let mut txn = environment.create_read_write_txn()?;
            let mut rocksdb_txn = rocksdb_store.create_read_write_txn()?;
            rocksdb_store.put(&mut rocksdb_txn, &root_hash, &root)?;
            trie_store.put(&mut txn, &root_hash, &root)?;
            txn.commit()?;
            environment.env().sync(true)?;
            root_hash
        };
        Ok(DbGlobalState {
            lmdb_environment: environment,
            lmdb_trie_store: trie_store,
            empty_root_hash,
            rocksdb_store,
        })
    }

    /// Creates a state from an existing environment, store, and root_hash.
    /// Intended to be used for testing.
    pub fn new(
        environment: Arc<LmdbEnvironment>,
        trie_store: Arc<LmdbTrieStore>,
        empty_root_hash: Digest,
        rocksdb_path: impl AsRef<Path>,
    ) -> Result<Self, error::Error> {
        let rocksdb_store =
            RocksDbStore::new(rocksdb_path, transaction_source::rocksdb_defaults())?;

        Ok(DbGlobalState {
            lmdb_environment: environment,
            lmdb_trie_store: trie_store,
            empty_root_hash,
            rocksdb_store,
        })
    }

    /// Creates an in-memory cache for changes written.
    pub fn create_scratch(&self) -> ScratchGlobalState {
        ScratchGlobalState::new(self.clone())
    }

    /// Write stored values to RocksDb.
    pub fn put_stored_values(
        &self,
        correlation_id: CorrelationId,
        prestate_hash: Digest,
        stored_values: HashMap<Key, StoredValue>,
    ) -> Result<Digest, error::Error> {
        put_stored_values::<_, _, error::Error>(
            &self.rocksdb_store,
            &self.rocksdb_store,
            correlation_id,
            prestate_hash,
            stored_values,
        )
        .map_err(Into::into)
    }
}

fn memoized_find_missing_descendants(
    value_bytes: Bytes,
    handle: RocksDb,
    rocksdb: RocksDb,
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
                find_missing_trie_keys(ptr, force, missing_trie_keys, &handle, &rocksdb)?;
            }
        }
        Trie::Extension { affix: _, pointer } => {
            find_missing_trie_keys(pointer, force, missing_trie_keys, &handle, &rocksdb)?;
        }
    }
    *time_in_missing_trie_keys += start_trie_keys.elapsed();
    Ok(())
}

fn find_missing_trie_keys(
    ptr: Pointer,
    force: bool,
    missing_trie_keys: &mut Vec<Digest>,
    handle: &RocksDb,
    rocksdb: &RocksDb,
) -> Result<(), error::Error> {
    let ptr = match ptr {
        Pointer::LeafPointer(pointer) | Pointer::NodePointer(pointer) => pointer,
    };
    if force {
        missing_trie_keys.push(ptr);
    } else {
        let existing = handle.read(rocksdb.clone(), &ptr.to_bytes()?)?;
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
        let txn = self.rocksdb_store.create_read_txn()?;
        let ret = match read::<Key, StoredValue, _, RocksDbStore, Self::Error>(
            correlation_id,
            &txn,
            &self.rocksdb_store,
            &self.root_hash,
            key,
        )? {
            ReadResult::Found(value) => Some(value),
            ReadResult::NotFound => None,
            ReadResult::RootNotFound => panic!("DbGlobalState has invalid root"),
        };
        txn.commit()?;
        Ok(ret)
    }

    fn read_with_proof(
        &self,
        correlation_id: CorrelationId,
        key: &Key,
    ) -> Result<Option<TrieMerkleProof<Key, StoredValue>>, Self::Error> {
        let txn = self.rocksdb_store.create_read_txn()?;
        let ret = match read_with_proof::<Key, StoredValue, _, _, Self::Error>(
            correlation_id,
            &txn,
            &self.rocksdb_store,
            &self.root_hash,
            key,
        )? {
            ReadResult::Found(value) => Some(value),
            ReadResult::NotFound => None,
            ReadResult::RootNotFound => panic!("DbGlobalState has invalid root"),
        };
        txn.commit()?;
        Ok(ret)
    }

    fn keys_with_prefix(
        &self,
        correlation_id: CorrelationId,
        prefix: &[u8],
    ) -> Result<Vec<Key>, Self::Error> {
        let txn = self.rocksdb_store.create_read_txn()?;
        let keys_iter = keys_with_prefix::<Key, StoredValue, _, _>(
            correlation_id,
            &txn,
            &self.rocksdb_store,
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

impl CommitProvider for DbGlobalState {
    fn commit(
        &self,
        correlation_id: CorrelationId,
        prestate_hash: Digest,
        effects: AdditiveMap<Key, Transform>,
    ) -> Result<Digest, Self::Error> {
        commit::<RocksDbStore, RocksDbStore, _, Self::Error>(
            &self.rocksdb_store,
            &self.rocksdb_store,
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
        let txn = self.rocksdb_store.create_read_txn()?;
        let rocksdb_store = self.rocksdb_store.clone();
        let maybe_root: Option<Trie<Key, StoredValue>> = rocksdb_store.get(&txn, &state_hash)?;
        let maybe_state = maybe_root.map(|_| DbGlobalStateView {
            root_hash: state_hash,
            rocksdb_store,
        });
        txn.commit()?;
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

        let txn = self.rocksdb_store.clone();
        let bytes = Store::<Digest, Trie<Digest, StoredValue>>::get_raw(
            &self.rocksdb_store,
            &txn.rocksdb,
            &trie_key,
        )?;

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

    fn put_trie(
        &self,
        correlation_id: CorrelationId,
        trie_bytes: &[u8],
    ) -> Result<Digest, Self::Error> {
        let mut txn = self.rocksdb_store.create_read_write_txn()?;
        let trie_store = self.rocksdb_store.clone();
        let trie_hash = put_trie::<Key, StoredValue, _, _, Self::Error>(
            correlation_id,
            &mut txn,
            &trie_store,
            trie_bytes,
        )?;
        txn.commit()?;
        Ok(trie_hash)
    }

    /// Finds all of the keys of missing descendant `Trie<K,V>` values.
    fn missing_trie_keys(
        &self,
        correlation_id: CorrelationId,
        trie_keys: Vec<Digest>,
    ) -> Result<Vec<Digest>, Self::Error> {
        let txn = self.rocksdb_store.create_read_txn()?;
        let trie_store = self.rocksdb_store.clone();
        let missing_descendants = missing_trie_keys::<Key, StoredValue, _, _, Self::Error>(
            correlation_id,
            &txn,
            &trie_store,
            trie_keys,
        )?;
        txn.commit()?;
        Ok(missing_descendants)
    }

    fn get_trie_full(
        &self,
        _correlation_id: CorrelationId,
        trie_key: &Digest,
    ) -> Result<Option<Bytes>, Self::Error> {
        let txn = self.rocksdb_store.clone();
        Store::<Digest, Trie<Digest, StoredValue>>::get_raw(
            &self.rocksdb_store,
            &txn.rocksdb,
            trie_key,
        )
    }
}

#[cfg(test)]
mod tests {
    use lmdb::DatabaseFlags;
    use tempfile::tempdir;

    use casper_hashing::Digest;
    use casper_types::{account::AccountHash, CLValue};

    use super::*;
    use crate::storage::{DEFAULT_TEST_MAX_DB_SIZE, DEFAULT_TEST_MAX_READERS};

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

    fn create_test_state() -> (DbGlobalState, Digest) {
        let correlation_id = CorrelationId::new();
        let lmdb_temp_dir = tempdir().unwrap();
        let rocksdb_temp_dir = tempdir().unwrap();
        let environment = Arc::new(
            LmdbEnvironment::new(
                lmdb_temp_dir.path(),
                DEFAULT_TEST_MAX_DB_SIZE,
                DEFAULT_TEST_MAX_READERS,
                true,
            )
            .unwrap(),
        );
        let trie_store =
            Arc::new(LmdbTrieStore::new(&environment, None, DatabaseFlags::empty()).unwrap());

        let engine_state =
            DbGlobalState::empty(environment, trie_store, &rocksdb_temp_dir.path()).unwrap();
        let mut current_root = engine_state.empty_root_hash;
        for TestPair { key, value } in create_test_pairs() {
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
