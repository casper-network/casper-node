//! Support for runtime configuration of the execution engine - as an integral property of the
//! `EngineState` instance.
use crate::shared::{chain_kind::ChainKind, system_config::SystemConfig, wasm_config::WasmConfig};

/// Default value for a maximum query depth configuration option.
pub const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;
/// Default value for maximum associated keys configuration option.
pub const DEFAULT_MAX_ASSOCIATED_KEYS: u32 = 100;
/// Default value for maximum runtime call stack height configuration option.
pub const DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT: u32 = 12;

/// The runtime configuration of the execution engine
#[derive(Debug, Copy, Clone)]
pub struct EngineConfig {
    /// Max query depth of the engine.
    pub(crate) max_query_depth: u64,
    /// Maximum number of associated keys (i.e. map of
    /// [`AccountHash`](casper_types::account::AccountHash)s to
    /// [`Weight`](casper_types::account::Weight)s) for a single account.
    max_associated_keys: u32,
    max_runtime_call_stack_height: u32,
    wasm_config: WasmConfig,
    system_config: SystemConfig,
    chain_kind: ChainKind,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            max_query_depth: DEFAULT_MAX_QUERY_DEPTH,
            max_associated_keys: DEFAULT_MAX_ASSOCIATED_KEYS,
            max_runtime_call_stack_height: DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
            wasm_config: WasmConfig::default(),
            system_config: SystemConfig::default(),
            chain_kind: ChainKind::default(),
        }
    }
}

impl EngineConfig {
    /// Returns the current max associated keys config.
    pub fn max_associated_keys(&self) -> u32 {
        self.max_associated_keys
    }

    /// Returns the current max runtime call stack height config.
    pub fn max_runtime_call_stack_height(&self) -> u32 {
        self.max_runtime_call_stack_height
    }

    /// Returns the current wasm config.
    pub fn wasm_config(&self) -> &WasmConfig {
        &self.wasm_config
    }

    /// Returns the current system config.
    pub fn system_config(&self) -> &SystemConfig {
        &self.system_config
    }

    /// Get the engine config's chain kind.
    #[must_use]
    pub fn chain_kind(&self) -> &ChainKind {
        &self.chain_kind
    }
}

/// This is a builder pattern applied to the [`EngineConfig`] structure to shield any changes to the
/// constructor, or contents of it from the rest of the system.
///
/// Any field that isn't specified will be defaulted.
#[derive(Default, Debug)]
pub struct EngineConfigBuilder {
    max_query_depth: Option<u64>,
    max_associated_keys: Option<u32>,
    max_runtime_call_stack_height: Option<u32>,
    wasm_config: Option<WasmConfig>,
    system_config: Option<SystemConfig>,
    chain_kind: Option<ChainKind>,
}

impl EngineConfigBuilder {
    /// Create new `EngineConfig` builder object.
    pub fn new() -> Self {
        EngineConfigBuilder::default()
    }

    /// Set a max query depth config option.
    pub fn with_max_query_depth(mut self, max_query_depth: u64) -> Self {
        self.max_query_depth = Some(max_query_depth);
        self
    }

    /// Set a max associated keys config option.
    pub fn with_max_associated_keys(mut self, max_associated_keys: u32) -> Self {
        self.max_associated_keys = Some(max_associated_keys);
        self
    }

    /// Set a max runtime call stack height option.
    pub fn with_max_runtime_call_stack_height(
        mut self,
        max_runtime_call_stack_height: u32,
    ) -> Self {
        self.max_runtime_call_stack_height = Some(max_runtime_call_stack_height);
        self
    }

    /// Set a new wasm config configuration option.
    pub fn with_wasm_config(mut self, wasm_config: WasmConfig) -> Self {
        self.wasm_config = Some(wasm_config);
        self
    }

    /// Set a new system config configuration option.
    pub fn with_system_config(mut self, system_config: SystemConfig) -> Self {
        self.system_config = Some(system_config);
        self
    }

    /// Sets new maximum wasm memory.
    pub fn with_wasm_max_stack_height(mut self, wasm_stack_height: u32) -> Self {
        let wasm_config = self.wasm_config.get_or_insert_with(WasmConfig::default);
        wasm_config.max_stack_height = wasm_stack_height;
        self
    }

    /// Sets new chain kind.
    pub fn with_chain_kind(mut self, chain_kind: ChainKind) -> Self {
        self.chain_kind = Some(chain_kind);
        self
    }

    /// Build a new [`EngineConfig`] object.
    pub fn build(self) -> EngineConfig {
        let max_query_depth = self.max_query_depth.unwrap_or(DEFAULT_MAX_QUERY_DEPTH);
        let max_associated_keys = self
            .max_associated_keys
            .unwrap_or(DEFAULT_MAX_ASSOCIATED_KEYS);
        let max_runtime_call_stack_height = self
            .max_runtime_call_stack_height
            .unwrap_or(DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT);
        let wasm_config = self.wasm_config.unwrap_or_default();
        let system_config = self.system_config.unwrap_or_default();
        let chain_kind = self.chain_kind.unwrap_or_default();
        EngineConfig {
            max_query_depth,
            max_associated_keys,
            max_runtime_call_stack_height,
            wasm_config,
            system_config,
            chain_kind,
        }
    }
}
