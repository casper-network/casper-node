use casper_node::{
    rpcs::{
        chain::{GetGlobalStateHash, GetGlobalStateHashParams},
        RpcWithOptionalParams,
    },
    types::BlockHash,
};

use crate::{error::Result, rpc::RpcClient};

impl RpcClient for GetGlobalStateHash {
    const RPC_METHOD: &'static str = Self::METHOD;
}

pub fn get_global_state_hash(
    node_address: String,
    maybe_block_hash: Option<BlockHash>,
) -> Result<String> {
    let response_value = match maybe_block_hash {
        Some(block_hash) => {
            let params = GetGlobalStateHashParams { block_hash };
            GetGlobalStateHash::request_with_map_params(&node_address, params)?
        }
        None => GetGlobalStateHash::request(&node_address)?,
    };
    Ok(serde_json::from_value::<String>(response_value)?)
}
