//! A database-backed trie store.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use rocksdb::{
    BlockBasedOptions, BoundColumnFamily, DBIteratorWithThreadMode, DBWithThreadMode, IteratorMode,
    MultiThreaded, Options,
};

use casper_types::{bytesrepr, bytesrepr::Bytes, Key, StoredValue};

use casper_hashing::Digest;

use crate::storage::{
    error,
    global_state::CommitError,
    store::{ErrorSource, Readable, Store, Writable},
    trie::Trie,
    trie_store::TrieStore,
};

/// Relative location (to storage) where rocksdb data will be stored.
pub const ROCKS_DB_DATA_DIR: &str = "rocksdb-data";

const ROCKS_DB_BLOCK_SIZE_BYTES: usize = 256 * 1024;
const ROCKS_DB_COMPRESSION_TYPE: rocksdb::DBCompressionType = rocksdb::DBCompressionType::Zstd;
const ROCKS_DB_COMPACTION_STYLE: rocksdb::DBCompactionStyle = rocksdb::DBCompactionStyle::Level;
const ROCKS_DB_ZSTD_MAX_DICT_BYTES: i32 = 256 * 1024;
const ROCKS_DB_MAX_LEVEL_FILE_SIZE_BYTES: u64 = 512 * 1024 * 1024;
const ROCKS_DB_MAX_OPEN_FILES: i32 = 768;
const ROCKS_DB_ZSTD_COMPRESSION_LEVEL: i32 = 3; // Default compression level
const ROCKS_DB_ZSTD_STRATEGY: i32 = 4; // 4: Lazy
const ROCKS_DB_WINDOW_BITS: i32 = -14;

/// Column family name for the v1 trie data store.
const TRIE_V1_COLUMN_FAMILY: &str = "trie_v1_column";

/// Column family name for tracking progress of data migration.
const LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY: &str = "lmdb_migrated_state_roots_column";

/// Cache used by the scratch trie.  The keys represent the hash of the trie being cached.  The
/// values represent:  1) A boolean, where `false` means the trie was _not_ written and `true` means
/// it was 2) A deserialized trie
pub(crate) type Cache = Arc<Mutex<HashMap<Digest, (bool, Trie<Key, StoredValue>)>>>;

/// Cached version of the trie store.
#[derive(Clone)]
pub(crate) struct ScratchTrieStore {
    pub(crate) cache: Cache,
    pub(crate) store: RocksDbStore,
}

impl ScratchTrieStore {
    /// Creates a new ScratchTrieStore.
    pub fn new(store: RocksDbStore) -> Self {
        Self {
            store,
            cache: Default::default(),
        }
    }

    /// Writes only tries which are both under the given `state_root` and dirty to the underlying db
    /// while maintaining the invariant that children must be written before parent nodes.
    pub fn write_root_to_db(self, state_root: Digest) -> Result<(), error::Error> {
        let store = self.store;
        let cache = &mut *self.cache.lock().map_err(|_| error::Error::Poison)?;

        let (is_root_dirty, root_trie) = cache
            .get(&state_root)
            .ok_or(CommitError::TrieNotFoundInCache(state_root))?;

        // Early exit if there is no work to do.
        if !is_root_dirty {
            return Ok(());
        }

        let mut tries_to_visit = vec![(state_root, root_trie, root_trie.iter_descendants())];

        while let Some((digest, current_trie, mut descendants_iterator)) = tries_to_visit.pop() {
            if let Some(descendant) = descendants_iterator.next() {
                tries_to_visit.push((digest, current_trie, descendants_iterator));
                // Only if a node is marked as dirty in the cache do we want to visit it's
                // descendants
                if let Some((true, child_trie)) = cache.get(&descendant) {
                    tries_to_visit.push((descendant, child_trie, child_trie.iter_descendants()));
                }
            } else {
                // We can write this node since it has no children, or they were already written.
                store.put(&digest, current_trie)?;
            }
        }

        Ok(())
    }
}

impl Store<Digest, Trie<Key, StoredValue>> for ScratchTrieStore {
    /// Puts a `value` into the store at `key` within a transaction, potentially returning an
    /// error of type `Self::Error` if that fails.
    fn put(&self, digest: &Digest, trie: &Trie<Key, StoredValue>) -> Result<(), Self::Error> {
        self.cache
            .lock()
            .map_err(|_| error::Error::Poison)?
            .insert(*digest, (true, trie.clone()));
        Ok(())
    }

    /// Returns an optional value (may exist or not) as read through a transaction, or an error
    /// of the associated `Self::Error` variety.
    fn get(&self, digest: &Digest) -> Result<Option<Trie<Key, StoredValue>>, Self::Error> {
        let maybe_trie = {
            self.cache
                .lock()
                .map_err(|_| error::Error::Poison)?
                .get(digest)
                .cloned()
        };
        match maybe_trie {
            Some((_, cached)) => Ok(Some(cached)),
            None => {
                let raw = self.get_raw(digest)?;
                match raw {
                    Some(bytes) => {
                        let value: Trie<Key, StoredValue> = bytesrepr::deserialize(bytes.into())?;
                        {
                            let store =
                                &mut *self.cache.lock().map_err(|_| error::Error::Poison)?;
                            if !store.contains_key(digest) {
                                store.insert(*digest, (false, value.clone()));
                            }
                        }
                        Ok(Some(value))
                    }
                    None => Ok(None),
                }
            }
        }
    }
}

/// Represents the state of a migration of a state root from lmdb to rocksdb.
pub enum RootMigration {
    /// Has the migration been not yet been started or completed
    NotStarted,
    /// Has the migration been left incomplete
    Partial,
    /// Has the migration been completed
    Complete,
}

impl RootMigration {
    /// Has the migration been not yet been started or completed
    pub fn is_not_started(&self) -> bool {
        matches!(self, RootMigration::NotStarted)
    }
    /// Has the migration been partially completed
    pub fn is_partial(&self) -> bool {
        matches!(self, RootMigration::Partial)
    }
    /// Has the migration been completed
    pub fn is_complete(&self) -> bool {
        matches!(self, RootMigration::Complete)
    }
}

impl ErrorSource for ScratchTrieStore {
    type Error = error::Error;
}

impl Readable for ScratchTrieStore {
    fn read(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error> {
        self.store.read(key)
    }
}

impl Writable for ScratchTrieStore {
    fn write(&self, key: &[u8], value: &[u8]) -> Result<(), Self::Error> {
        self.store.write(key, value)
    }
}

impl ErrorSource for RocksDbStore {
    type Error = error::Error;
}

impl Readable for RocksDbStore {
    fn read(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error> {
        let cf = self.trie_column_family()?;
        Ok(self.db.get_cf(&cf, key)?.map(|some| {
            let value = some.as_ref();
            Bytes::from(value)
        }))
    }
}

impl Writable for RocksDbStore {
    fn write(&self, key: &[u8], value: &[u8]) -> Result<(), Self::Error> {
        let cf = self.trie_column_family()?;
        let _result = self.db.put_cf(&cf, key, value)?;
        Ok(())
    }
}

/// Environment for rocksdb.
#[derive(Clone)]
pub struct RocksDbStore {
    db: Arc<DBWithThreadMode<MultiThreaded>>,
    pub(crate) path: PathBuf,
}

const ACTIVE_COLUMN_FAMILIES: &[&str] = &[
    TRIE_V1_COLUMN_FAMILY,
    LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY,
];

impl RocksDbStore {
    /// Create a new environment for RocksDB.
    pub fn new(path: impl AsRef<Path>) -> Result<RocksDbStore, rocksdb::Error> {
        Self::new_with_opts(path, Self::rocksdb_defaults())
    }

    /// Create a new environment for RocksDB with the specified options.
    pub fn new_with_opts(
        path: impl AsRef<Path>,
        rocksdb_opts: Options,
    ) -> Result<RocksDbStore, rocksdb::Error> {
        let db = Arc::new(DBWithThreadMode::<MultiThreaded>::open_cf(
            &rocksdb_opts,
            path.as_ref(),
            ACTIVE_COLUMN_FAMILIES.to_vec(),
        )?);

        Ok(RocksDbStore {
            db,
            path: path.as_ref().to_path_buf(),
        })
    }

    /// Default constructor for rocksdb options.
    pub fn rocksdb_defaults() -> Options {
        let mut factory_opts = BlockBasedOptions::default();
        factory_opts.set_block_size(ROCKS_DB_BLOCK_SIZE_BYTES);

        let mut db_opts = Options::default();
        db_opts.set_block_based_table_factory(&factory_opts);

        db_opts.set_compression_type(ROCKS_DB_COMPRESSION_TYPE);
        db_opts.set_compression_options(
            ROCKS_DB_WINDOW_BITS,
            ROCKS_DB_ZSTD_COMPRESSION_LEVEL,
            ROCKS_DB_ZSTD_STRATEGY,
            ROCKS_DB_ZSTD_MAX_DICT_BYTES,
        );

        // seems to lead to a sporadic segfault within rocksdb compaction
        // const ROCKS_DB_ZSTD_MAX_TRAIN_BYTES: i32 = 1024 * 1024; // 1 MB
        // db_opts.set_zstd_max_train_bytes(ROCKS_DB_ZSTD_MAX_TRAIN_BYTES);

        db_opts.set_compaction_style(ROCKS_DB_COMPACTION_STYLE);
        db_opts.set_max_bytes_for_level_base(ROCKS_DB_MAX_LEVEL_FILE_SIZE_BYTES);
        db_opts.set_max_open_files(ROCKS_DB_MAX_OPEN_FILES);

        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        // recommended to match # of cores on host.
        db_opts.increase_parallelism(num_cpus::get() as i32);

        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        db_opts
    }

    /// Check if a state root has been marked as migrated from lmdb to rocksdb.
    pub(crate) fn get_root_migration_state(
        &self,
        state_root: &[u8],
    ) -> Result<RootMigration, error::Error> {
        let lmdb_migration_column = self.lmdb_tries_migrated_column()?;
        let migration_state = self.db.get_cf(&lmdb_migration_column, state_root)?;
        Ok(match migration_state {
            Some(state) if state.is_empty() => RootMigration::Complete,
            Some(state) if state.get(0) == Some(&1u8) => RootMigration::Partial,
            _ => RootMigration::NotStarted,
        })
    }

    /// Marks a state root as started migration from lmdb to rocksdb.
    pub(crate) fn mark_state_root_migration_incomplete(
        &self,
        state_root: &[u8],
    ) -> Result<(), error::Error> {
        let lmdb_migration_column = self.lmdb_tries_migrated_column()?;
        self.db.put_cf(&lmdb_migration_column, state_root, &[1u8])?;
        Ok(())
    }

    /// Marks a state root as fully migrated from lmdb to rocksdb.
    pub(crate) fn mark_state_root_migration_completed(
        &self,
        state_root: &[u8],
    ) -> Result<(), error::Error> {
        let lmdb_migration_column = self.lmdb_tries_migrated_column()?;
        self.db.put_cf(&lmdb_migration_column, state_root, &[])?;
        Ok(())
    }

    /// Trie V1 column family.
    fn trie_column_family(&self) -> Result<Arc<BoundColumnFamily<'_>>, error::Error> {
        self.db.cf_handle(TRIE_V1_COLUMN_FAMILY).ok_or_else(|| {
            error::Error::UnableToOpenColumnFamily(TRIE_V1_COLUMN_FAMILY.to_string())
        })
    }

    /// Column family tracking state roots migrated from lmdb (supports safely resuming if the node
    /// were to be stopped during a migration).
    fn lmdb_tries_migrated_column(&self) -> Result<Arc<BoundColumnFamily<'_>>, error::Error> {
        self.db
            .cf_handle(LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY)
            .ok_or_else(|| {
                error::Error::UnableToOpenColumnFamily(
                    LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY.to_string(),
                )
            })
    }

    /// Trie store iterator.
    pub fn trie_store_iterator<'a: 'b, 'b>(
        &'a self,
    ) -> Result<DBIteratorWithThreadMode<'b, DBWithThreadMode<MultiThreaded>>, error::Error> {
        let cf_handle = self.trie_column_family()?;
        Ok(self.db.iterator_cf(&cf_handle, IteratorMode::Start))
    }

    /// Return the path to the backing rocksdb files.
    pub(crate) fn path(&self) -> PathBuf {
        self.path.clone()
    }
}

impl TrieStore<Key, StoredValue> for ScratchTrieStore {}
impl<K, V> Store<Digest, Trie<K, V>> for RocksDbStore {}
impl<K, V> TrieStore<K, V> for RocksDbStore {}
