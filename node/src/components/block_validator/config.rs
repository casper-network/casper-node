use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// Configuration options for block validation.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct Config {
    pub max_completed_entries: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_completed_entries: 3,
        }
    }
}
