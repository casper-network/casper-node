use std::path::PathBuf;

use datasize::DataSize;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use tempfile::TempDir;
use tracing::warn;

use casper_execution_engine::shared::utils;

const QUALIFIER: &str = "io";
const ORGANIZATION: &str = "CasperLabs";
const APPLICATION: &str = "casper-node";

const DEFAULT_MAX_BLOCK_STORE_SIZE: usize = 483_183_820_800; // 450 GiB
const DEFAULT_MAX_DEPLOY_STORE_SIZE: usize = 322_122_547_200; // 300 GiB
const DEFAULT_MAX_BLOCK_HEIGHT_STORE_SIZE: usize = 10_485_100; // 10 MiB
const DEFAULT_MAX_CHAINSPEC_STORE_SIZE: usize = 1_073_741_824; // 1 GiB
const DEFAULT_MAX_BLOCK_PROPOSER_STATE_STORE_SIZE: usize = 1_073_741_824; // 1 GiB

#[cfg(test)]
const DEFAULT_TEST_MAX_DB_SIZE: usize = 52_428_800; // 50 MiB

/// On-disk storage configuration.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The path to the folder where any files created or read by the storage component will exist.
    ///
    /// If the folder doesn't exist, it and any required parents will be created.
    ///
    /// If unset via the configuration file, the value must be provided via the CLI.
    path: PathBuf,
    /// The maximum size of the database to use for the block store.
    ///
    /// Defaults to 483,183,820,800 == 450 GiB.
    ///
    /// The size should be a multiple of the OS page size.
    max_block_store_size: Option<usize>,
    /// The maximum size of the database to use for the deploy store.
    ///
    /// Defaults to 322,122,547,200 == 300 GiB.
    ///
    /// The size should be a multiple of the OS page size.
    max_deploy_store_size: Option<usize>,
    /// The maximum size of the database to use for the block-height store.
    ///
    /// Defaults to 10,485,100 == 10 MiB.
    ///
    /// The size should be a multiple of the OS page size.
    max_block_height_store_size: Option<usize>,
    /// The maximum size of the database to use for the chainspec store.
    ///
    /// Defaults to 1,073,741,824 == 1 GiB.
    ///
    /// The size should be a multiple of the OS page size.
    max_chainspec_store_size: Option<usize>,
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
            max_block_store_size: Some(DEFAULT_TEST_MAX_DB_SIZE),
            max_deploy_store_size: Some(DEFAULT_TEST_MAX_DB_SIZE),
            max_block_height_store_size: Some(DEFAULT_TEST_MAX_DB_SIZE),
            max_chainspec_store_size: Some(DEFAULT_TEST_MAX_DB_SIZE),
        };
        (config, tempdir)
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.path.clone()
    }

    pub(crate) fn max_block_store_size(&self) -> usize {
        let value = self
            .max_block_store_size
            .unwrap_or(DEFAULT_MAX_BLOCK_STORE_SIZE);
        utils::check_multiple_of_page_size(value);
        value
    }

    pub(crate) fn max_deploy_store_size(&self) -> usize {
        let value = self
            .max_deploy_store_size
            .unwrap_or(DEFAULT_MAX_DEPLOY_STORE_SIZE);
        utils::check_multiple_of_page_size(value);
        value
    }

    pub(crate) fn max_block_height_store_size(&self) -> usize {
        let value = self
            .max_block_height_store_size
            .unwrap_or(DEFAULT_MAX_BLOCK_HEIGHT_STORE_SIZE);
        utils::check_multiple_of_page_size(value);
        value
    }

    pub(crate) fn max_chainspec_store_size(&self) -> usize {
        let value = self
            .max_chainspec_store_size
            .unwrap_or(DEFAULT_MAX_CHAINSPEC_STORE_SIZE);
        utils::check_multiple_of_page_size(value);
        value
    }

    pub(crate) fn max_block_proposer_state_store_size(&self) -> usize {
        let value = self
            .max_block_store_size
            .unwrap_or(DEFAULT_MAX_BLOCK_PROPOSER_STATE_STORE_SIZE);
        utils::check_multiple_of_page_size(value);
        value
    }

    fn default_path() -> PathBuf {
        ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .map(|project_dirs| project_dirs.data_dir().to_path_buf())
            .unwrap_or_else(|| {
                warn!("failed to get project dir - falling back to current dir");
                PathBuf::from(".")
            })
    }
}

impl Default for Config {
    fn default() -> Self {
        let path = Self::default_path();

        Config {
            path,
            max_block_store_size: Some(DEFAULT_MAX_BLOCK_STORE_SIZE),
            max_deploy_store_size: Some(DEFAULT_MAX_DEPLOY_STORE_SIZE),
            max_block_height_store_size: Some(DEFAULT_MAX_BLOCK_HEIGHT_STORE_SIZE),
            max_chainspec_store_size: Some(DEFAULT_MAX_CHAINSPEC_STORE_SIZE),
        }
    }
}
