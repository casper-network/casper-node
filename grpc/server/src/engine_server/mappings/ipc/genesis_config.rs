use std::convert::{TryFrom, TryInto};

use casper_node::components::contract_runtime::core::engine_state::genesis::GenesisConfig;

use crate::engine_server::{ipc::ChainSpec_GenesisConfig, mappings::MappingError};

impl From<GenesisConfig> for ChainSpec_GenesisConfig {
    fn from(genesis_config: GenesisConfig) -> Self {
        let mut pb_genesis_config = ChainSpec_GenesisConfig::new();

        pb_genesis_config.set_name(genesis_config.name().to_string());
        pb_genesis_config.set_timestamp(genesis_config.timestamp());
        pb_genesis_config.set_protocol_version(genesis_config.protocol_version().into());
        pb_genesis_config.set_ee_config(genesis_config.take_ee_config().into());
        pb_genesis_config
    }
}

impl TryFrom<ChainSpec_GenesisConfig> for GenesisConfig {
    type Error = MappingError;

    fn try_from(mut pb_genesis_config: ChainSpec_GenesisConfig) -> Result<Self, Self::Error> {
        let name = pb_genesis_config.take_name();
        let timestamp = pb_genesis_config.get_timestamp();
        let protocol_version = pb_genesis_config.take_protocol_version().into();
        let ee_config = pb_genesis_config.take_ee_config().try_into()?;
        Ok(GenesisConfig::new(
            name,
            timestamp,
            protocol_version,
            ee_config,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_server::mappings::test_utils;

    #[test]
    fn round_trip() {
        let genesis_config = rand::random();
        test_utils::protobuf_round_trip::<GenesisConfig, ChainSpec_GenesisConfig>(genesis_config);
    }
}
