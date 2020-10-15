use casper_execution_engine::core::engine_state::ExecutableDeployItem;
use casper_node::types::Deploy;

use super::{DeployExt, DeployParams};

use crate::error::Result;

pub fn make_deploy(
    maybe_output_path: Option<&str>,
    deploy_params: DeployParams,
    payment: ExecutableDeployItem,
    session: ExecutableDeployItem,
) -> Result<()> {
    let deploy = Deploy::with_payment_and_session(deploy_params, payment, session);
    deploy.write_deploy(maybe_output_path)
}
