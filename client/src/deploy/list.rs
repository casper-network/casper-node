use semver::Version;
use serde::{Deserialize, Serialize};

use casper_node::{
    rpcs::{
        chain::{GetBlock, GetBlockParams, GetBlockResult},
        RpcWithOptionalParams,
    },
    types::{BlockHash, DeployHash},
};

use crate::{error::Result, rpc::RpcClient};

pub struct ListDeploys {}

impl RpcClient for ListDeploys {
    const RPC_METHOD: &'static str = GetBlock::METHOD;
}

/// Result for "chain_get_block" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct ListDeploysResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The deploy hashes of the block, if found.
    pub deploy_hashes: Option<Vec<DeployHash>>,
}

impl From<GetBlockResult> for ListDeploysResult {
    fn from(get_block_result: GetBlockResult) -> Self {
        ListDeploysResult {
            api_version: get_block_result.api_version,
            deploy_hashes: get_block_result
                .block
                .map(|block| block.deploy_hashes().clone()),
        }
    }
}

pub fn list_deploys(
    node_address: String,
    maybe_block_hash: Option<BlockHash>,
) -> Result<ListDeploysResult> {
    let response_value = match maybe_block_hash {
        Some(block_hash) => {
            let params = GetBlockParams { block_hash };
            ListDeploys::request_with_map_params(&node_address, params)?
        }
        None => ListDeploys::request(&node_address)?,
    };
    let get_block_result: GetBlockResult = serde_json::from_value(response_value)?;
    let result = ListDeploysResult::from(get_block_result);
    Ok(result)
}
