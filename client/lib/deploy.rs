use std::{
    fs::File,
    io::{self, BufReader, Write},
};

use semver::Version;
use serde::{Deserialize, Serialize};

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

use crate::{error::Result, rpc::RpcClient};

/// SendDeploy allows sending a deploy to the node.
pub(crate) struct SendDeploy;

/// Transfer allows transferring an amount between accounts.
pub(crate) struct Transfer {}

/// ListDeploys gets a list of the deploys within a block.
pub(crate) struct ListDeploys {}

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
        Some(output_path) => File::create(&output_path).map(|file| {
            let write: Box<dyn Write> = Box::new(file);
            write
        }),
        None => Ok(Box::new(io::stdout())),
    }
}

/// `DeployParams` are used as a helper to construct a `Deploy` with
/// `DeployExt::with_payment_and_session`.
pub struct DeployParams {
    /// The secret key for this `Deploy`.
    pub secret_key: SecretKey,

    /// The creation timestamp of this `Deploy`.
    pub timestamp: Timestamp,

    /// The time to live for this `Deploy`.
    pub ttl: TimeDiff,

    /// The gas price for this `Deploy`.
    pub gas_price: u64,

    /// A list of other `Deploy`s (hashes) that this `Deploy` depends upon.
    pub dependencies: Vec<DeployHash>,

    /// The name of the chain this `Deploy` will be considered for inclusion in.
    pub chain_name: String,
}

/// An extension trait that adds some client-specific functionality to `Deploy`.
pub(super) trait DeployExt {
    /// Constructs a `Deploy`.
    fn with_payment_and_session(
        params: DeployParams,
        payment: ExecutableDeployItem,
        session: ExecutableDeployItem,
    ) -> Deploy;

    /// Writes the `Deploy` to a file, or if `maybe_path` is `None`, `stdout`.
    fn write_deploy(&self, maybe_path: Option<&str>) -> Result<()>;

    /// Reads a `Deploy` from the file at `input_path`.
    fn read_deploy(input_path: &str) -> Result<Deploy>;

    /// Reads a `Deploy` from the file at `input_path`, signs it, then writes it back to a file at
    /// `maybe_output_path` if `Some` or `stdout` if `None`.
    fn sign_deploy_file(
        input_path: &str,
        secret_key: SecretKey,
        maybe_output_path: Option<&str>,
    ) -> Result<()>;

    /// Constructs a `Deploy` and writes it to a file at `maybe_output_path` if `Some` or `stdout`
    /// if `None`.
    fn make_deploy(
        maybe_output_path: Option<&str>,
        deploy_params: DeployParams,
        payment: ExecutableDeployItem,
        session: ExecutableDeployItem,
    ) -> Result<()>;
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

    fn write_deploy(&self, maybe_path: Option<&str>) -> Result<()> {
        let mut out = output_or_stdout(&maybe_path)?;
        let content = serde_json::to_string_pretty(self)?;
        Ok(out.write_all(content.as_bytes())?)
    }

    fn read_deploy(input_path: &str) -> Result<Deploy> {
        let reader = BufReader::new(File::open(&input_path)?);
        Ok(serde_json::from_reader(reader)?)
    }

    fn make_deploy(
        maybe_output_path: Option<&str>,
        deploy_params: DeployParams,
        payment: ExecutableDeployItem,
        session: ExecutableDeployItem,
    ) -> Result<()> {
        let deploy = Deploy::with_payment_and_session(deploy_params, payment, session);
        deploy.write_deploy(maybe_output_path)
    }

    fn sign_deploy_file(
        input_path: &str,
        secret_key: SecretKey,
        maybe_output_path: Option<&str>,
    ) -> Result<()> {
        let mut deploy = Deploy::read_deploy(input_path)?;
        let mut rng = rand::thread_rng();
        deploy.sign(&secret_key, &mut rng);
        deploy.write_deploy(maybe_output_path)?;
        Ok(())
    }
}
