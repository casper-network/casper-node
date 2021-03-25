const DEFAULT_QUERY_DEPTH_LIMIT: u64 = 5;

/// The runtime configuration of the execution engine
#[derive(Debug, Copy, Clone)]
pub struct EngineConfig {
    pub(crate) query_depth_limit: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            query_depth_limit: DEFAULT_QUERY_DEPTH_LIMIT,
        }
    }
}

impl EngineConfig {
    /// Creates a new engine configuration with provided parameters.
    pub fn new(query_depth_limit: u64) -> EngineConfig {
        EngineConfig { query_depth_limit }
    }
}
