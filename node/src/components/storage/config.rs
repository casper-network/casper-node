use std::path::PathBuf;

use crate::components::contract_runtime::shared::page_size;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use tracing::warn;

const QUALIFIER: &str = "io";
const ORGANIZATION: &str = "CasperLabs";
const APPLICATION: &str = "casperlabs-node";

const DEFAULT_MAX_BLOCK_STORE_SIZE: usize = 483_183_820_800; // 450 GiB
const DEFAULT_MAX_DEPLOY_STORE_SIZE: usize = 322_122_547_200; // 300 GiB
const DEFAULT_MAX_CHAINSPEC_STORE_SIZE: usize = 1_073_741_824; // 1 GiB

const DEFAULT_TEST_MAX_DB_SIZE: usize = 52_428_800; // 50 MiB

/// On-disk storage configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    /// The path to the folder where any files created or read by the storage component will exist.
    ///
    /// If the folder doesn't exist, it and any required parents will be created.
    ///
    /// Defaults to:
    /// * Linux: `$XDG_DATA_HOME/casperlabs-node` or `$HOME/.local/share/casperlabs-node`, e.g.
    ///   /home/alice/.local/share/casperlabs-node
    /// * macOS: `$HOME/Library/Application Support/io.CasperLabs.casperlabs-node`, e.g.
    ///   /Users/Alice/Library/Application Support/io.CasperLabs.casperlabs-node
    /// * Windows: `{FOLDERID_RoamingAppData}\CasperLabs\casperlabs-node\data` e.g.
    ///   C:\Users\Alice\AppData\Roaming\CasperLabs\casperlabs-node\data
    pub path: PathBuf,
    /// Sets the maximum size of the database to use for the block store.
    ///
    /// Defaults to 483,183,820,800 == 450 GiB.
    ///
    /// The size should be a multiple of the OS page size.
    #[serde(deserialize_with = "page_size::deserialize_page_size_multiple")]
    pub max_block_store_size: usize,
    /// Sets the maximum size of the database to use for the deploy store.
    ///
    /// Defaults to 322,122,547,200 == 300 GiB.
    ///
    /// The size should be a multiple of the OS page size.
    #[serde(deserialize_with = "page_size::deserialize_page_size_multiple")]
    pub max_deploy_store_size: usize,
    /// Sets the maximum size of the database to use for the chainspec store.
    ///
    /// Defaults to 1,073,741,824 == 1 GiB.
    ///
    /// The size should be a multiple of the OS page size.
    #[serde(deserialize_with = "page_size::deserialize_page_size_multiple")]
    pub max_chainspec_store_size: usize,
}

impl Config {
    /// Returns a default `Config` suitable for tests, along with a `TempDir` which must be kept
    /// alive for the duration of the test since its destructor removes the dir from the filesystem.
    #[allow(unused)]
    pub(crate) fn default_for_tests() -> (Self, TempDir) {
        let tempdir = tempfile::tempdir().expect("should get tempdir");
        let path = tempdir.path().to_path_buf();

        let config = Config {
            path,
            max_block_store_size: DEFAULT_TEST_MAX_DB_SIZE,
            max_deploy_store_size: DEFAULT_TEST_MAX_DB_SIZE,
            max_chainspec_store_size: DEFAULT_TEST_MAX_DB_SIZE,
        };
        (config, tempdir)
    }
}

impl Default for Config {
    fn default() -> Self {
        let path = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .map(|project_dirs| project_dirs.data_dir().to_path_buf())
            .unwrap_or_else(|| {
                warn!("failed to get project dir - falling back to current dir");
                PathBuf::from(".")
            });

        Config {
            path,
            max_block_store_size: DEFAULT_MAX_BLOCK_STORE_SIZE,
            max_deploy_store_size: DEFAULT_MAX_DEPLOY_STORE_SIZE,
            max_chainspec_store_size: DEFAULT_MAX_CHAINSPEC_STORE_SIZE,
        }
    }
}
