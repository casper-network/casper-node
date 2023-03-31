use datasize::DataSize;
use serde::Deserialize;

use crate::{
    logging::LoggingConfig, types::NodeConfig, BlockAccumulatorConfig, BlockSynchronizerConfig,
    ConsensusConfig, ContractRuntimeConfig, DeployBufferConfig, DiagnosticsPortConfig,
    EventStreamServerConfig, FetcherConfig, GossipConfig, NetworkConfig, RestServerConfig,
    RpcServerConfig, SpeculativeExecConfig, StorageConfig, UpgradeWatcherConfig,
};

/// Root configuration.
#[derive(DataSize, Debug, Default, Deserialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Config values for the node.
    pub node: NodeConfig,
    /// Config values for logging.
    pub logging: LoggingConfig,
    /// Config values for consensus.
    pub consensus: ConsensusConfig,
    /// Config values for network.
    pub network: NetworkConfig,
    /// Config values for the event stream server.
    pub event_stream_server: EventStreamServerConfig,
    /// Config values for the REST server.
    pub rest_server: RestServerConfig,
    /// Config values for the Json-RPC server.
    pub rpc_server: RpcServerConfig,
    /// Config values for speculative execution.
    pub speculative_exec_server: SpeculativeExecConfig,
    /// Config values for storage.
    pub storage: StorageConfig,
    /// Config values for gossip.
    pub gossip: GossipConfig,
    /// Config values for fetchers.
    pub fetcher: FetcherConfig,
    /// Config values for the contract runtime.
    pub contract_runtime: ContractRuntimeConfig,
    /// Config values for the deploy buffer.
    pub deploy_buffer: DeployBufferConfig,
    /// Config values for the diagnostics port.
    pub diagnostics_port: DiagnosticsPortConfig,
    /// Config values for the block accumulator.
    pub block_accumulator: BlockAccumulatorConfig,
    /// Config values for the block synchronizer.
    pub block_synchronizer: BlockSynchronizerConfig,
    /// Config values for the upgrade watcher.
    pub upgrade_watcher: UpgradeWatcherConfig,
}
