use casper_types::bytesrepr::Bytes;
use rocksdb::{BlockBasedOptions, Options};

/// DB implementation of transaction source.
pub mod db;
/// In-memory implementation of transaction source.
pub mod in_memory;

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
const ROCKS_DB_TRIE_V1_COLUMN_FAMILY: &str = "trie_v1_column";
/// A transaction which can be committed or aborted.
pub trait Transaction: Sized {
    /// An error which can occur while reading or writing during a transaction,
    /// or committing the transaction.
    type Error;

    /// An entity which is being read from or written to during a transaction.
    type Handle;

    /// Commits the transaction.
    fn commit(self) -> Result<(), Self::Error>;

    /// Aborts the transaction.
    ///
    /// Any pending operations will not be saved.
    fn abort(self) {
        unimplemented!("Abort operations should be performed in Drop implementations.")
    }
}

/// A transaction with the capability to read from a given [`Handle`](Transaction::Handle).
pub trait Readable: Transaction {
    /// Returns the value from the corresponding key from a given [`Transaction::Handle`].
    fn read(&self, handle: Self::Handle, key: &[u8]) -> Result<Option<Bytes>, Self::Error>;
}

/// A transaction with the capability to write to a given [`Handle`](Transaction::Handle).
pub trait Writable: Transaction {
    /// Inserts a key-value pair into a given [`Transaction::Handle`].
    fn write(&mut self, handle: Self::Handle, key: &[u8], value: &[u8]) -> Result<(), Self::Error>;
}

/// A source of transactions e.g. values that implement [`Readable`]
/// and/or [`Writable`].
pub trait TransactionSource<'a> {
    /// An error which can occur while creating a read or read-write
    /// transaction.
    type Error;

    /// An entity which is being read from or written to during a transaction.
    type Handle;

    /// Represents the type of read transactions.
    type ReadTransaction: Readable<Error = Self::Error, Handle = Self::Handle>;

    /// Represents the type of read-write transactions.
    type ReadWriteTransaction: Readable<Error = Self::Error, Handle = Self::Handle>
        + Writable<Error = Self::Error, Handle = Self::Handle>;

    /// Creates a read transaction.
    fn create_read_txn(&'a self) -> Result<Self::ReadTransaction, Self::Error>;

    /// Creates a read-write transaction.
    fn create_read_write_txn(&'a self) -> Result<Self::ReadWriteTransaction, Self::Error>;
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
