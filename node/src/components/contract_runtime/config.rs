use serde::{Deserialize, Serialize};

use crate::components::storage;

const DEFAULT_MAX_GLOBAL_STATE_SIZE: usize = 805_306_368_000; // 750 GiB
const DEFAULT_USE_SYSTEM_CONTRACTS: bool = false;
const DEFAULT_ENABLE_BONDING: bool = false;

/// Contract runtime configuration.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Config {
    /// Whether to use system contracts or not.  Defaults to false.
    use_system_contracts: Option<bool>,
    /// Whether to enable bonding or not.  Defaults to false.
    enable_bonding: Option<bool>,
    /// The maximum size of the database to use for the global state store.
    ///
    /// Defaults to 805,306,368,000 == 750 GiB.
    ///
    /// The size should be a multiple of the OS page size.
    max_global_state_size: Option<usize>,
}

impl Config {
    pub(crate) fn use_system_contracts(&self) -> bool {
        self.use_system_contracts
            .unwrap_or(DEFAULT_USE_SYSTEM_CONTRACTS)
    }

    pub(crate) fn enable_bonding(&self) -> bool {
        self.enable_bonding.unwrap_or(DEFAULT_ENABLE_BONDING)
    }

    pub(crate) fn max_global_state_size(&self) -> usize {
        let value = self
            .max_global_state_size
            .unwrap_or(DEFAULT_MAX_GLOBAL_STATE_SIZE);
        storage::check_multiple_of_page_size(value);
        value
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            use_system_contracts: Some(DEFAULT_USE_SYSTEM_CONTRACTS),
            enable_bonding: Some(DEFAULT_ENABLE_BONDING),
            max_global_state_size: Some(DEFAULT_MAX_GLOBAL_STATE_SIZE),
        }
    }
}
