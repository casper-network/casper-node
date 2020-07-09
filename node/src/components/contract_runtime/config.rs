use crate::components::contract_runtime::shared::page_size;
use serde::{Deserialize, Serialize};

// 750 GiB = 805306368000 bytes
// page size on x86_64 linux = 4096 bytes
// 805306368000 / 4096 = 196608000
const DEFAULT_PAGES: usize = 196_608_000;

/// Contract runtime configuration.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Config {
    /// Storage
    pub use_system_contracts: bool,
    /// Enables
    pub enable_bonding: bool,
    /// Map size
    #[serde(deserialize_with = "page_size::deserialize_page_size_multiple")]
    pub map_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            use_system_contracts: false,
            enable_bonding: false,
            map_size: DEFAULT_PAGES * *page_size::PAGE_SIZE,
        }
    }
}
