use casper_node::{
    rpcs::{
        chain::{GetBlock, GetBlockParams, GetBlockResult},
        RpcWithOptionalParams,
    },
    types::BlockHash,
};

use crate::{error::Result, rpc::RpcClient};

impl RpcClient for GetBlock {
    const RPC_METHOD: &'static str = Self::METHOD;
}

pub fn get_block(
    node_address: &str,
    maybe_block_hash: Option<BlockHash>,
) -> Result<GetBlockResult> {
    let response_value = match maybe_block_hash {
        Some(block_hash) => {
            let params = GetBlockParams { block_hash };
            GetBlock::request_with_map_params(&node_address, params)?
        }
        None => GetBlock::request(&node_address)?,
    };
    Ok(serde_json::from_value::<GetBlockResult>(response_value)?)
}
