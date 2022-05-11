use casper_types::bytesrepr::{self, Bytes};
use rocksdb::{BlockBasedOptions, Options};

/// DB implementation of transaction source.
pub mod db;

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

/// Base trait for db operations that can raise an error.
pub trait ErrorSource: Sized {
    /// Type of error held by the trait.
    type Error: From<bytesrepr::Error>;
}

/// Represents a store that can be read from.
pub trait Readable: ErrorSource {
    /// Returns the value from the corresponding key.
    fn read(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error>;
}

/// Represents a store that can be written to.
pub trait Writable: ErrorSource {
    /// Inserts a key-value pair.
    fn write(&self, key: &[u8], value: &[u8]) -> Result<(), Self::Error>;
}

/// Default constructor for rocksdb options.
pub(super) fn rocksdb_defaults() -> Options {
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
