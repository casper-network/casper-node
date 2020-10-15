use std::{
    fs::File,
    io::{self, Write},
};

use casper_execution_engine::core::engine_state::ExecutableDeployItem;
use casper_node::{
    crypto::asymmetric_key::SecretKey,
    rpcs::{
        account::PutDeploy,
        chain::{GetBlock, GetBlockResult},
        info::GetDeploy,
        RpcWithOptionalParams, RpcWithParams,
    },
    types::{Deploy, DeployHash, TimeDiff, Timestamp},
};
use io::BufReader;
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{error::Result, rpc::RpcClient};

pub struct SendDeploy;
pub struct Transfer {}
pub struct ListDeploys {}

impl RpcClient for PutDeploy {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl RpcClient for GetDeploy {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl RpcClient for SendDeploy {
    const RPC_METHOD: &'static str = PutDeploy::METHOD;
}

impl RpcClient for Transfer {
    const RPC_METHOD: &'static str = PutDeploy::METHOD;
}

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

/// Creates a Write trait object for File or Stdout respective to the path value passed
/// Stdout is used when None
fn output_or_stdout(maybe_path: &Option<&str>) -> io::Result<Box<dyn Write>> {
    match maybe_path {
        Some(output_path) => File::create(&output_path).map(|f| Box::new(f) as Box<dyn Write>),
        None => Ok(Box::new(std::io::stdout()) as Box<dyn Write>),
    }
}

pub struct DeployParams {
    pub secret_key: SecretKey,
    pub timestamp: Timestamp,
    pub ttl: TimeDiff,
    pub gas_price: u64,
    pub dependencies: Vec<DeployHash>,
    pub chain_name: String,
}

pub trait DeployExt {
    fn with_payment_and_session(
        params: DeployParams,
        payment: ExecutableDeployItem,
        session: ExecutableDeployItem,
    ) -> Deploy;
    fn write_deploy(&self, maybe_path: Option<&str>) -> Result<()>;
    fn read_deploy(input_path: &str) -> Result<Deploy>;
}

impl DeployExt for Deploy {
    fn with_payment_and_session(
        params: DeployParams,
        payment: ExecutableDeployItem,
        session: ExecutableDeployItem,
    ) -> Deploy {
        let DeployParams {
            timestamp,
            ttl,
            gas_price,
            dependencies,
            chain_name,
            secret_key,
        } = params;
        let mut rng = rand::thread_rng();
        Deploy::new(
            timestamp,
            ttl,
            gas_price,
            dependencies,
            chain_name,
            payment,
            session,
            &secret_key,
            &mut rng,
        )
    }

    /// Write the deploy to a file, or if maybe_path is None, stdout
    fn write_deploy(&self, maybe_path: Option<&str>) -> Result<()> {
        let mut out = output_or_stdout(&maybe_path)?;
        let content = serde_json::to_string_pretty(self)?;
        Ok(out.write_all(content.as_bytes())?)
    }

    /// Read a deploy from a file
    fn read_deploy(input_path: &str) -> Result<Deploy> {
        let reader = BufReader::new(File::open(&input_path)?);
        Ok(serde_json::from_reader(reader)?)
    }
}

pub fn make_deploy(
    maybe_output_path: Option<&str>,
    deploy_params: DeployParams,
    payment: ExecutableDeployItem,
    session: ExecutableDeployItem,
) -> Result<()> {
    let deploy = Deploy::with_payment_and_session(deploy_params, payment, session);
    deploy.write_deploy(maybe_output_path)
}

pub fn sign_deploy_file(
    input_path: &str,
    secret_key: SecretKey,
    output_path: Option<&str>,
) -> Result<()> {
    let mut deploy = Deploy::read_deploy(input_path)?;
    let mut rng = rand::thread_rng();
    deploy.sign(&secret_key, &mut rng);
    deploy.write_deploy(output_path)?;
    Ok(())
}
