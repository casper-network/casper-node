use std::path::PathBuf;

use datasize::DataSize;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use tempfile::TempDir;

/// One Gibibyte.
const GIB: usize = 1024 * 1024 * 1024;
/// Default max block store size.
const DEFAULT_MAX_BLOCK_STORE_SIZE: usize = 450 * GIB;
/// Default max deploy store size.
const DEFAULT_MAX_DEPLOY_STORE_SIZE: usize = 300 * GIB;
/// Default max deploy metadata store size.
const DEFAULT_MAX_DEPLOY_METADATA_STORE_SIZE: usize = 300 * GIB;
/// Default max state store size.
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
    pub(super) max_block_store_size: usize,
    /// The maximum size of the database to use for the deploy store.
    ///
    /// The size should be a multiple of the OS page size.
    pub(super) max_deploy_store_size: usize,
    /// The maximum size of the database to use for the deploy metadata store.
    ///
    /// The size should be a multiple of the OS page size.
    pub(super) max_deploy_metadata_store_size: usize,
    /// The maximum size of the database to use for the component state store.
    ///
    /// The size should be a multiple of the OS page size.
    pub(super) max_state_store_size: usize,
    /// Whether or not memory deduplication is enabled.
    pub(super) enable_mem_deduplication: bool,
    /// How many loads before memory duplication checks for dead references.
    pub(super) mem_pool_prune_interval: u16,
    /// Maximum number of parallel synchronous storage tasks to spawn.
    pub(super) max_sync_tasks: u16,
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
            max_sync_tasks: 32,
        }
    }
}

impl Config {
    /// Returns a default `Config` suitable for tests, along with a `TempDir` which must be kept
    /// alive for the duration of the test since its destructor removes the dir from the filesystem.
    #[cfg(test)]
    pub(crate) fn default_for_tests() -> (Self, TempDir) {
        let tempdir = tempfile::tempdir().expect("should get tempdir");
        let path = tempdir.path().join("lmdb");

        let config = Config {
            path,
            ..Default::default()
        };
        (config, tempdir)
    }
}
