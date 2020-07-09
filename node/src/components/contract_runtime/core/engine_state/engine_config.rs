/// The runtime configuration of the execution engine
#[derive(Debug, Copy, Clone, Default)]
pub struct EngineConfig {
    // feature flags go here
    use_system_contracts: bool,
    enable_bonding: bool,
}

impl EngineConfig {
    /// Creates a new engine configuration with default parameters.
    pub fn new() -> EngineConfig {
        Default::default()
    }

    pub fn use_system_contracts(self) -> bool {
        self.use_system_contracts
    }

    pub fn with_use_system_contracts(mut self, use_system_contracts: bool) -> EngineConfig {
        self.use_system_contracts = use_system_contracts;
        self
    }

    pub fn enable_bonding(self) -> bool {
        self.enable_bonding
    }

    pub fn with_enable_bonding(mut self, enable_bonding: bool) -> EngineConfig {
        self.enable_bonding = enable_bonding;
        self
    }
}
