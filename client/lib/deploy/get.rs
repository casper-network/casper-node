use casper_node::{
    rpcs::{
        info::{GetDeploy, GetDeployParams, GetDeployResult},
        RpcWithParams,
    },
    types::DeployHash,
};

use crate::{error::Result, rpc::RpcClient};

impl RpcClient for GetDeploy {
    const RPC_METHOD: &'static str = Self::METHOD;
}

pub fn get_deploy(node_address: String, deploy_hash: DeployHash) -> Result<GetDeployResult> {
    let params = GetDeployParams { deploy_hash };
    let response_value = GetDeploy::request_with_map_params(&node_address, params)?;
    Ok(serde_json::from_value::<GetDeployResult>(response_value)?)
}
