use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{
    logging::LoggingConfig, types::NodeConfig, ConsensusConfig, ContractRuntimeConfig,
    DeployAcceptorConfig, EventStreamServerConfig, FetcherConfig, GossipConfig, RestServerConfig,
    RpcServerConfig, SmallNetworkConfig, StorageConfig,
};

/// Root configuration.
#[derive(DataSize, Debug, Default, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Node configuration.
    pub node: NodeConfig,
    /// Logging configuration.
    pub logging: LoggingConfig,
    /// Consensus configuration.
    pub consensus: ConsensusConfig,
    /// Network configuration.
    pub network: SmallNetworkConfig,
    /// Event stream API server configuration.
    pub event_stream_server: EventStreamServerConfig,
    /// REST API server configuration.
    pub rest_server: RestServerConfig,
    /// RPC API server configuration.
    pub rpc_server: RpcServerConfig,
    /// On-disk storage configuration.
    pub storage: StorageConfig,
    /// Gossip protocol configuration.
    pub gossip: GossipConfig,
    /// Fetcher configuration.
    pub fetcher: FetcherConfig,
    /// Contract runtime configuration.
    pub contract_runtime: ContractRuntimeConfig,
    /// Deploy acceptor configuration.
    pub deploy_acceptor: DeployAcceptorConfig,
}
