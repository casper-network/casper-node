use casper_execution_engine::core::engine_state::ExecutableDeployItem;
use casper_node::{
    crypto::asymmetric_key::SecretKey,
    types::{DeployHash, TimeDiff, Timestamp},
};

mod get;
mod list;
mod make;
mod put;
mod send;
mod sign;
mod transfer;

pub use get::get_deploy;
pub use list::list_deploys;
pub use make::make_deploy;
pub use put::put_deploy;
pub use send::send_deploy_file;
pub use sign::sign_deploy_file;
pub use transfer::transfer;

pub struct DeployParams {
    pub secret_key: SecretKey,
    pub timestamp: Timestamp,
    pub ttl: TimeDiff,
    pub gas_price: u64,
    pub dependencies: Vec<DeployHash>,
    pub chain_name: String,
}
use std::{
    fs::File,
    io::{self, Write},
};

use crate::error::Result;
use casper_node::types::Deploy;
use io::BufReader;

/// Creates a Write trait object for File or Stdout respective to the path value passed
/// Stdout is used when None
fn output_or_stdout(maybe_path: &Option<&str>) -> io::Result<Box<dyn Write>> {
    match maybe_path {
        Some(output_path) => File::create(&output_path).map(|f| Box::new(f) as Box<dyn Write>),
        None => Ok(Box::new(std::io::stdout()) as Box<dyn Write>),
    }
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
