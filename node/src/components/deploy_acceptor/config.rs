use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// Configuration options for fetching.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct Config {
    verify_accounts: bool,
}

impl Config {
    /// Constructor for deploy_acceptor config.
    pub fn new(verify_accounts: bool) -> Self {
        Config { verify_accounts }
    }

    /// Get verify_accounts setting.
    pub(crate) fn verify_accounts(&self) -> bool {
        self.verify_accounts
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            verify_accounts: true,
        }
    }
}
