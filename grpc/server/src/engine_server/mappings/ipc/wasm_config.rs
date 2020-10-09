use std::convert::{TryFrom, TryInto};

use casper_execution_engine::shared::wasm_config::WasmConfig;

use crate::engine_server::{ipc, mappings::MappingError};

impl From<WasmConfig> for ipc::ChainSpec_WasmConfig {
    fn from(wasm_config: WasmConfig) -> Self {
        let mut pb_wasmconfig = ipc::ChainSpec_WasmConfig::new();

        pb_wasmconfig.set_initial_mem(wasm_config.initial_memory);
        pb_wasmconfig.set_max_stack_height(wasm_config.max_stack_height);
        pb_wasmconfig.set_opcode_costs(wasm_config.opcode_costs().into());
        pb_wasmconfig.set_storage_costs(wasm_config.storage_costs().into());
        pb_wasmconfig.set_host_function_costs(wasm_config.take_host_function_costs().into());

        pb_wasmconfig
    }
}

impl TryFrom<ipc::ChainSpec_WasmConfig> for WasmConfig {
    type Error = MappingError;

    fn try_from(mut pb_wasm_config: ipc::ChainSpec_WasmConfig) -> Result<Self, Self::Error> {
        Ok(WasmConfig::new(
            pb_wasm_config.initial_mem,
            pb_wasm_config.max_stack_height,
            pb_wasm_config.take_opcode_costs().into(),
            pb_wasm_config.take_storage_costs().into(),
            pb_wasm_config.take_host_function_costs().try_into()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_execution_engine::shared::wasm_config::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(wasm_costs in gens::wasm_config_arb()) {
            test_utils::protobuf_round_trip::<WasmConfig, ipc::ChainSpec_WasmConfig>(wasm_costs);
        }
    }
}
