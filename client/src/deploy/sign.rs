use casper_node::{crypto::asymmetric_key::SecretKey, types::Deploy};

use super::DeployExt;
use crate::error::Result;

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
