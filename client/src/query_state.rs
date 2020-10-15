use casper_node::{
    crypto::hash::Digest,
    rpcs::{
        state::{GetItem, GetItemParams, GetItemResult},
        RpcWithParams,
    },
};

use crate::{error::Result, rpc::RpcClient};

impl RpcClient for GetItem {
    const RPC_METHOD: &'static str = <Self as RpcWithParams>::METHOD;
}

pub fn get_item(
    node_address: String,
    global_state_hash: Digest,
    key: String,
    path: Vec<String>,
) -> Result<GetItemResult> {
    let params = GetItemParams {
        global_state_hash,
        key,
        path,
    };
    let response_value = GetItem::request_with_map_params(&node_address, params)?;
    Ok(serde_json::from_value::<GetItemResult>(response_value)?)
}
