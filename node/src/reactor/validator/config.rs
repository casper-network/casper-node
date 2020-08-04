use serde::{Deserialize, Serialize};

use crate::{
    logging::LoggingConfig, types::NodeConfig, ApiServerConfig, ConsensusConfig,
    ContractRuntimeConfig, GossipConfig, SmallNetworkConfig, StorageConfig,
    ROOT_VALIDATOR_LISTENING_PORT,
};

/// Root configuration.
#[derive(Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Node configuration.
    pub node: NodeConfig,
    /// Logging configuration.
    pub logging: LoggingConfig,
    /// Consensus configuration.
    pub consensus: ConsensusConfig,
    /// Network configuration for the validator-only network.
    pub validator_net: SmallNetworkConfig,
    /// Network configuration for the HTTP API.
    pub http_server: ApiServerConfig,
    /// On-disk storage configuration.
    pub storage: StorageConfig,
    /// Gossip protocol configuration.
    pub gossip: GossipConfig,
    /// Contract runtime configuration.
    pub contract_runtime: ContractRuntimeConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            node: NodeConfig::default(),
            logging: LoggingConfig::default(),
            consensus: ConsensusConfig::default(),
            validator_net: SmallNetworkConfig::default_on_port(ROOT_VALIDATOR_LISTENING_PORT),
            http_server: ApiServerConfig::default(),
            storage: StorageConfig::default(),
            gossip: GossipConfig::default(),
            contract_runtime: ContractRuntimeConfig::default(),
        }
    }
}
