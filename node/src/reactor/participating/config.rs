use datasize::DataSize;
use serde::Deserialize;

use crate::{
    logging::LoggingConfig, types::NodeConfig, BlockProposerConfig, ConsensusConfig,
    ContractRuntimeConfig, DiagnosticsPortConfig, EventStreamServerConfig, FetcherConfig,
    GossipConfig, RestServerConfig, RpcServerConfig, SmallNetworkConfig, StorageConfig,
};

/// Root configuration.
#[derive(DataSize, Debug, Default, Deserialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    /// Node fast-sync configuration.
    pub(crate) node: NodeConfig,
    /// Logging configuration.
    pub(crate) logging: LoggingConfig,
    /// Consensus configuration.
    pub(crate) consensus: ConsensusConfig,
    /// Network configuration.
    pub(crate) network: SmallNetworkConfig,
    /// Event stream API server configuration.
    pub(crate) event_stream_server: EventStreamServerConfig,
    /// REST API server configuration.
    pub(crate) rest_server: RestServerConfig,
    /// RPC API server configuration.
    pub(crate) rpc_server: RpcServerConfig,
    /// On-disk storage configuration.
    pub(crate) storage: StorageConfig,
    /// Gossip protocol configuration.
    pub(crate) gossip: GossipConfig,
    /// Fetcher configuration.
    pub(crate) fetcher: FetcherConfig,
    /// Contract runtime configuration.
    pub(crate) contract_runtime: ContractRuntimeConfig,
    /// Block proposer configuration.
    #[serde(default)]
    pub(crate) block_proposer: BlockProposerConfig,
    /// Diagnostics port configuration.
    pub(crate) diagnostics_port: DiagnosticsPortConfig,
}
