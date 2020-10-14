use casper_node::rpcs::{
    state::{GetBalance, GetBalanceParams, GetBalanceResult},
    RpcWithParams,
};

use crate::{error::Result, rpc::RpcClient};

impl RpcClient for GetBalance {
    const RPC_METHOD: &'static str = Self::METHOD;
}

pub fn get_balance(node_address: String, params: GetBalanceParams) -> Result<GetBalanceResult> {
    let response_value = GetBalance::request_with_map_params(&node_address, params)?;
    Ok(serde_json::from_value::<GetBalanceResult>(response_value)?)
}
