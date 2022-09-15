use std::time::Duration;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

// TODO: 60s might be too aggressive
const DEFAULT_EXPIRY_CHECK_SECONDS: u64 = 60u64;

/// Configuration options for deploy_buffer.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
pub(crate) struct Config {
    expiry_check_seconds: u64,
}

impl Config {
    pub(crate) fn expiry(&self) -> Duration {
        Duration::from_secs(self.expiry_check_seconds)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            expiry_check_seconds: DEFAULT_EXPIRY_CHECK_SECONDS,
        }
    }
}
