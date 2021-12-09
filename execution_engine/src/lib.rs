//! The engine which executes smart contracts on the Casper network.

#![doc(html_root_url = "https://docs.rs/casper-execution-engine/1.4.3")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![warn(
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unused_qualifications
)]

pub mod config;
pub mod core;
pub mod shared;
/// Storage for the execution engine.
pub mod storage;

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
pub const ROCKS_DB_TRIE_V1_COLUMN_FAMILY: &str = "trie_v1_column";

/// Default constructor for rocksdb options.
pub fn rocksdb_defaults() -> rocksdb::Options {
    let mut factory_opts = rocksdb::BlockBasedOptions::default();
    factory_opts.set_block_size(ROCKS_DB_BLOCK_SIZE_BYTES);

    let mut db_opts = rocksdb::Options::default();
    db_opts.set_block_based_table_factory(&factory_opts);

    db_opts.set_compression_type(ROCKS_DB_COMPRESSION_TYPE);
    db_opts.set_compression_options(
        ROCKS_DB_WINDOW_BITS,
        ROCKS_DB_ZSTD_COMPRESSION_LEVEL,
        ROCKS_DB_ZSTD_STRATEGY,
        ROCKS_DB_ZSTD_MAX_DICT_BYTES,
    );

    // seems to lead to a sporatic segfault within rocksdb compaction
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
