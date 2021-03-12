use serde::{Deserialize, Serialize};

use datasize::DataSize;

use super::round_success_meter::config::Config as RSMConfig;

/// Highway-specific configuration.
/// NOTE: This is *NOT* protocol configuration that has to be the same on all nodes.
#[derive(DataSize, Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Config {
    pub round_success_meter: RSMConfig,
}
