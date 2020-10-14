use casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItem;
use casper_node::{
    crypto::asymmetric_key::PublicKey,
    rpcs::{
        account::{PutDeploy, PutDeployParams},
        RpcWithParams,
    },
    types::Deploy,
};
use casper_types::{RuntimeArgs, U512, URef, bytesrepr::ToBytes};

use crate::{deploy::DeployParams, error::Result, rpc::RpcClient};

use super::DeployExt;

pub struct Transfer {}

impl RpcClient for Transfer {
    const RPC_METHOD: &'static str = PutDeploy::METHOD;
}

pub fn transfer(
    node_address: String,
    amount: U512,
    source_purse: Option<URef>, // TODO un-option and multivariate
    target_account: Option<PublicKey>,
    target_purse: Option<URef>,
    deploy_params: DeployParams,
    payment: ExecutableDeployItem,
) -> Result<()> {
    const TRANSFER_ARG_AMOUNT: &str = "amount";
    const TRANSFER_ARG_SOURCE: &str = "source";
    const TRANSFER_ARG_TARGET: &str = "target";
    let mut transfer_args = RuntimeArgs::new();
    transfer_args.insert(TRANSFER_ARG_AMOUNT, amount);
    if let Some(source_purse) = source_purse {
        transfer_args.insert(TRANSFER_ARG_SOURCE, source_purse);
    }
    match (target_account, target_purse) {
        (Some(target_account), None) => {
            let target_account_hash = target_account.to_account_hash().value();
            transfer_args.insert(TRANSFER_ARG_TARGET, target_account_hash);
        }
        (None, Some(target_purse)) => {
            transfer_args.insert(TRANSFER_ARG_TARGET, target_purse);
        }
        _ => unreachable!("should have a target"),
    }
    let session = ExecutableDeployItem::Transfer {
        args: transfer_args.to_bytes()?,
    };
    let deploy = Deploy::with_payment_and_session(deploy_params, payment, session);
    let params = PutDeployParams { deploy };
    let _ = Transfer::request_with_map_params(&node_address, params)?;
    Ok(())
}
