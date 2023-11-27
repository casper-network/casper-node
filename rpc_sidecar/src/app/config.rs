use casper_rpc_sidecar::{NodeClientConfig, RpcConfig, SpeculativeExecConfig};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    pub rpc_server: RpcConfig,
    pub speculative_exec_server: Option<SpeculativeExecConfig>,
    pub node_client: NodeClientConfig,
}
