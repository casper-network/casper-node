use std::convert::TryFrom;

use casper_execution_engine::shared::system_config::SystemConfig;

use crate::engine_server::{ipc, mappings::MappingError};

impl From<SystemConfig> for ipc::ChainSpec_SystemConfig {
    fn from(system_config: SystemConfig) -> Self {
        let mut pb_system_config = ipc::ChainSpec_SystemConfig::new();

        pb_system_config.set_wasmless_transfer_cost(system_config.wasmless_transfer_cost());

        pb_system_config
    }
}

impl TryFrom<ipc::ChainSpec_SystemConfig> for SystemConfig {
    type Error = MappingError;

    fn try_from(pb_system_config: ipc::ChainSpec_SystemConfig) -> Result<Self, Self::Error> {
        Ok(SystemConfig::new(
            pb_system_config.get_wasmless_transfer_cost(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_execution_engine::shared::system_config::{gens, SystemConfig};

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(system_config in gens::system_config_arb()) {
            test_utils::protobuf_round_trip::<SystemConfig, ipc::ChainSpec_SystemConfig>(system_config);
        }
    }
}
