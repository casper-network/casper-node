const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;

/// The runtime configuration of the execution engine
#[derive(Debug, Copy, Clone)]
pub struct EngineConfig {
    pub(crate) max_query_depth: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            max_query_depth: DEFAULT_MAX_QUERY_DEPTH,
        }
    }
}

impl EngineConfig {
    /// Creates a new engine configuration with provided parameters.
    pub fn new(max_query_depth: u64) -> EngineConfig {
        EngineConfig { max_query_depth }
    }
}
