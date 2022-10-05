use datasize::DataSize;
use serde::Deserialize;

use crate::{
    logging::LoggingConfig, types::NodeConfig, BlockAccumulatorConfig, BlockSynchronizerConfig,
    ConsensusConfig, ContractRuntimeConfig, DeployBufferConfig, DiagnosticsPortConfig,
    EventStreamServerConfig, FetcherConfig, GossipConfig, RestServerConfig, RpcServerConfig,
    SmallNetworkConfig, SpeculativeExecConfig, StorageConfig,
};

/// Root configuration.
#[derive(DataSize, Debug, Default, Deserialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) node: NodeConfig,
    pub(crate) logging: LoggingConfig,
    pub(crate) consensus: ConsensusConfig,
    pub(crate) network: SmallNetworkConfig,
    pub(crate) event_stream_server: EventStreamServerConfig,
    pub(crate) rest_server: RestServerConfig,
    pub(crate) rpc_server: RpcServerConfig,
    pub(crate) speculative_exec_server: SpeculativeExecConfig,
    pub(crate) storage: StorageConfig,
    pub(crate) gossip: GossipConfig,
    pub(crate) fetcher: FetcherConfig,
    pub(crate) contract_runtime: ContractRuntimeConfig,
    pub(crate) deploy_buffer: DeployBufferConfig,
    pub(crate) diagnostics_port: DiagnosticsPortConfig,
    pub(crate) block_accumulator: BlockAccumulatorConfig,
    pub(crate) block_synchronizer: BlockSynchronizerConfig,
}
