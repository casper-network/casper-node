/// The runtime configuration of the execution engine
#[derive(Debug, Copy, Clone, Default)]
pub struct EngineConfig {
    // feature flags go here
}

impl EngineConfig {
    /// Creates a new engine configuration with default parameters.
    pub fn new() -> EngineConfig {
        Default::default()
    }
}
