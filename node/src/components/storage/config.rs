use std::path::PathBuf;

use datasize::DataSize;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use tempfile::TempDir;

const GIB: usize = 1024 * 1024 * 1024;
const DEFAULT_MAX_BLOCK_STORE_SIZE: usize = 450 * GIB;
const DEFAULT_MAX_DEPLOY_STORE_SIZE: usize = 300 * GIB;
const DEFAULT_MAX_DEPLOY_METADATA_STORE_SIZE: usize = 300 * GIB;
const DEFAULT_MAX_STATE_STORE_SIZE: usize = 10 * GIB;

/// On-disk storage configuration.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The path to the folder where any files created or read by the storage component will exist.
    ///
    /// If the folder doesn't exist, it and any required parents will be created.
    pub path: PathBuf,
    /// The maximum size of the database to use for the block store.
    ///
    /// The size should be a multiple of the OS page size.
    pub max_block_store_size: usize,
    /// The maximum size of the database to use for the deploy store.
    ///
    /// The size should be a multiple of the OS page size.
    pub max_deploy_store_size: usize,
    /// The maximum size of the database to use for the deploy metadata store.
    ///
    /// The size should be a multiple of the OS page size.
    pub max_deploy_metadata_store_size: usize,
    /// The maximum size of the database to use for the component state store.
    ///
    /// The size should be a multiple of the OS page size.
    pub max_state_store_size: usize,
    /// Whether or not memory deduplication is enabled.
    pub enable_mem_deduplication: bool,
    /// How many loads before memory duplication checks for dead references.
    pub mem_pool_prune_interval: u16,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // No one should be instantiating a config with storage set to default.
            path: "/dev/null".into(),
            max_block_store_size: DEFAULT_MAX_BLOCK_STORE_SIZE,
            max_deploy_store_size: DEFAULT_MAX_DEPLOY_STORE_SIZE,
            max_deploy_metadata_store_size: DEFAULT_MAX_DEPLOY_METADATA_STORE_SIZE,
            max_state_store_size: DEFAULT_MAX_STATE_STORE_SIZE,
            enable_mem_deduplication: true,
            mem_pool_prune_interval: 4096,
        }
    }
}

impl Config {
    /// Returns a `Config` suitable for tests, along with a `TempDir` which must be kept alive for
    /// the duration of the test since its destructor removes the dir from the filesystem.
    ///
    /// `size_multiplier` is used to multiply the default DB sizes.
    #[cfg(test)]
    pub(crate) fn new_for_tests(size_multiplier: u8) -> (Self, TempDir) {
        if size_multiplier == 0 {
            panic!("size_multiplier cannot be zero");
        }
        let tempdir = tempfile::tempdir().expect("should get tempdir");
        let path = tempdir.path().join("lmdb");

        let config = Config {
            path,
            max_block_store_size: 1024 * 1024 * size_multiplier as usize,
            max_deploy_store_size: 1024 * 1024 * size_multiplier as usize,
            max_deploy_metadata_store_size: 1024 * 1024 * size_multiplier as usize,
            max_state_store_size: 12 * 1024 * size_multiplier as usize,
            ..Default::default()
        };
        (config, tempdir)
    }
}
