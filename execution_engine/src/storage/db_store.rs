///! RocksDb Generic Database backed store.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use rocksdb::{
    BlockBasedOptions, BoundColumnFamily, DBWithThreadMode,
    MultiThreaded, Options,
};

use super::error;

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

/// Working set column famliy.
const WORKING_SET_COLUMN_FAMILY: &str = "working_set";

const ACTIVE_COLUMN_FAMILIES: &[&str] = &[
    TRIE_V1_COLUMN_FAMILY,
    LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY,
    WORKING_SET_COLUMN_FAMILY,
];

/// Generic data store backed by RocksDb.
#[derive(Clone)]
pub struct DbStore {
    pub(crate) db: Arc<DBWithThreadMode<MultiThreaded>>,
    pub(crate) path: PathBuf,
}

impl DbStore {
    /// Create a new store backed by RocksDB.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, rocksdb::Error> {
        Self::new_with_opts(path, Self::rocksdb_defaults())
    }

    /// Create a new store backed by RocksDB with the specified options.
    pub fn new_with_opts(
        path: impl AsRef<Path>,
        rocksdb_opts: Options,
    ) -> Result<Self, rocksdb::Error> {
        let db = Arc::new(DBWithThreadMode::<MultiThreaded>::open_cf(
            &rocksdb_opts,
            path.as_ref(),
            ACTIVE_COLUMN_FAMILIES.to_vec(),
        )?);

        Ok(DbStore {
            db,
            path: path.as_ref().to_path_buf(),
        })
    }

    /// Trie V1 column family.
    pub(crate) fn trie_column_family(&self) -> Result<Arc<BoundColumnFamily<'_>>, error::Error> {
        self.db.cf_handle(TRIE_V1_COLUMN_FAMILY).ok_or_else(|| {
            error::Error::UnableToOpenColumnFamily(TRIE_V1_COLUMN_FAMILY.to_string())
        })
    }

    /// Column family tracking state roots migrated from lmdb (supports safely resuming if the node
    /// were to be stopped during a migration).
    pub(crate) fn lmdb_tries_migrated_column(
        &self,
    ) -> Result<Arc<BoundColumnFamily<'_>>, error::Error> {
        self.db
            .cf_handle(LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY)
            .ok_or_else(|| {
                error::Error::UnableToOpenColumnFamily(
                    LMDB_MIGRATED_STATE_ROOTS_COLUMN_FAMILY.to_string(),
                )
            })
    }

    /// Working set column family.
    pub fn working_set_column_family(&self) -> Result<Arc<BoundColumnFamily<'_>>, error::Error> {
        self.db.cf_handle(WORKING_SET_COLUMN_FAMILY).ok_or_else(|| {
            error::Error::UnableToOpenColumnFamily(WORKING_SET_COLUMN_FAMILY.to_string())
        })
    }

    /// Return the path to the backing rocksdb files.
    pub(crate) fn path(&self) -> PathBuf {
        self.path.clone()
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
}
