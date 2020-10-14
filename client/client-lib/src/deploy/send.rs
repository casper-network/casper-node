use casper_node::{
    rpcs::{
        account::{PutDeploy, PutDeployParams},
        RpcWithParams,
    },
    types::Deploy,
};

use super::DeployExt;
use crate::{error::Result, rpc::RpcClient};

pub struct SendDeploy;

impl RpcClient for SendDeploy {
    const RPC_METHOD: &'static str = PutDeploy::METHOD;
}

pub fn send_deploy_file(node_address: &str, input_path: &str) -> Result<()> {
    let deploy = Deploy::read_deploy(&input_path)?;
    let params = PutDeployParams { deploy };
    let _ = SendDeploy::request_with_map_params(node_address, params)?;
    Ok(())
}
