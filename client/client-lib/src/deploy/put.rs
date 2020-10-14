use casper_node::{
    rpcs::{
        account::{PutDeploy, PutDeployParams},
        RpcWithParams,
    },
    types::Deploy,
};

use crate::{error::Result, rpc::RpcClient};

impl RpcClient for PutDeploy {
    const RPC_METHOD: &'static str = Self::METHOD;
}

pub fn put_deploy(node_address: String, deploy: Deploy) -> Result<()> {
    let params = PutDeployParams { deploy };
    let _response_value = PutDeploy::request_with_map_params(&node_address, params)?;
    Ok(())
}
