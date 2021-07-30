use crate::shared::{system_config::SystemConfig, wasm_config::WasmConfig};

pub const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;

/// The runtime configuration of the execution engine
#[derive(Debug, Copy, Clone)]
pub struct EngineConfig {
    pub(crate) max_query_depth: u64,
    wasm_config: WasmConfig,
    system_config: SystemConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            max_query_depth: DEFAULT_MAX_QUERY_DEPTH,
            wasm_config: WasmConfig::default(),
            system_config: SystemConfig::default(),
        }
    }
}

impl EngineConfig {
    /// Creates a new engine configuration with provided parameters.
    pub fn new(
        max_query_depth: u64,
        wasm_config: WasmConfig,
        system_config: SystemConfig,
    ) -> EngineConfig {
        EngineConfig {
            max_query_depth,
            wasm_config,
            system_config,
        }
    }

    /// Returns the current wasm config.
    pub fn wasm_config(&self) -> &WasmConfig {
        &self.wasm_config
    }

    /// Returns the current system config.
    pub fn system_config(&self) -> &SystemConfig {
        &self.system_config
    }
}
