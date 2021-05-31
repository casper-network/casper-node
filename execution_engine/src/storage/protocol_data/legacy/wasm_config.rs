use casper_types::bytesrepr::{self, FromBytes};

use crate::shared::wasm_config::WasmConfig;

use super::{
    host_function_costs::LegacyHostFunctionCosts, opcode_costs::LegacyOpcodeCosts,
    storage_costs::LegacyStorageCosts,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LegacyWasmConfig(WasmConfig);

impl From<LegacyWasmConfig> for WasmConfig {
    fn from(legacy_wasm_config: LegacyWasmConfig) -> Self {
        legacy_wasm_config.0
    }
}

impl FromBytes for LegacyWasmConfig {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (max_memory, rem) = u32::from_bytes(bytes)?;
        let (max_stack_height, rem) = u32::from_bytes(rem)?;
        let (legacy_opcode_costs, rem) = LegacyOpcodeCosts::from_bytes(rem)?;
        let (legacy_storage_costs, rem) = LegacyStorageCosts::from_bytes(rem)?;
        let (legacy_host_function_costs, rem) = LegacyHostFunctionCosts::from_bytes(rem)?;

        let wasm_config = WasmConfig::new(
            max_memory,
            max_stack_height,
            legacy_opcode_costs.into(),
            legacy_storage_costs.into(),
            legacy_host_function_costs.into(),
        );

        Ok((LegacyWasmConfig(wasm_config), rem))
    }
}
