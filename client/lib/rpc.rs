use futures::executor;
use jsonrpc_lite::{JsonRpc, Params};
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Map, Value};

use casper_execution_engine::core::engine_state::ExecutableDeployItem;
use casper_node::{
    crypto::{asymmetric_key::PublicKey, hash::Digest},
    rpcs::{
        account::{PutDeploy, PutDeployParams},
        chain::{GetBlock, GetBlockParams, GetGlobalStateHash, GetGlobalStateHashParams},
        info::{GetDeploy, GetDeployParams},
        state::{GetBalance, GetBalanceParams, GetItem, GetItemParams},
        RPC_API_PATH,
    },
    types::{BlockHash, Deploy, DeployHash},
};
use casper_types::{bytesrepr::ToBytes, RuntimeArgs, URef, U512};

use crate::{
    deploy::{DeployExt, DeployParams, ListDeploys, SendDeploy, Transfer},
    error::{Error, Result},
};

/// Struct representing a single JSON-RPC call to the `casper-node`.
#[derive(Debug, Default)]
pub struct RpcCall {
    rpc_id: u32,
    verbose: bool,
}

/// RpcCall encapsulates calls made to the `casper-node` service via JSON-RPC.
impl RpcCall {
    /// Create a new instance, specifying an `rpc_id` for RPC-ID as required by the JSON-RPC
    /// specification. When `verbose` is `true`, requests will be printed to `stdout`.
    pub fn new(rpc_id: u32, verbose: bool) -> Self {
        Self { rpc_id, verbose }
    }

    /// Gets a deploy from the node.
    pub fn get_deploy(self, node_address: String, deploy_hash: DeployHash) -> Result<JsonRpc> {
        let params = GetDeployParams { deploy_hash };
        GetDeploy::request_with_map_params(self.verbose, &node_address, self.rpc_id, params)
    }

    /// Queries the node for an item at `key`, given a `path` and a `global_state_hash`.
    pub fn get_item(
        self,
        node_address: String,
        global_state_hash: Digest,
        key: String,
        path: Vec<String>,
    ) -> Result<JsonRpc> {
        let params = GetItemParams {
            global_state_hash,
            key,
            path,
        };
        GetItem::request_with_map_params(self.verbose, &node_address, self.rpc_id, params)
    }

    /// Queries the node for the most recent `global_state_hash`, or as of a given block hash if
    /// `maybe_block_hash` is provided.
    pub fn get_global_state_hash(
        self,
        node_address: String,
        maybe_block_hash: Option<BlockHash>,
    ) -> Result<JsonRpc> {
        match maybe_block_hash {
            Some(block_hash) => {
                let params = GetGlobalStateHashParams { block_hash };
                GetGlobalStateHash::request_with_map_params(
                    self.verbose,
                    &node_address,
                    self.rpc_id,
                    params,
                )
            }
            None => GetGlobalStateHash::request(self.verbose, &node_address, self.rpc_id),
        }
    }

    /// Get the balance from an account.
    pub fn get_balance(self, node_address: String, params: GetBalanceParams) -> Result<JsonRpc> {
        GetBalance::request_with_map_params(self.verbose, &node_address, self.rpc_id, params)
    }

    /// Transfer an amount between accounts.
    #[allow(clippy::too_many_arguments)]
    pub fn transfer(
        self,
        node_address: String,
        amount: U512,
        source_purse: Option<URef>, // TODO un-option and multivariate
        target_account: Option<PublicKey>,
        target_purse: Option<URef>,
        deploy_params: DeployParams,
        payment: ExecutableDeployItem,
    ) -> Result<JsonRpc> {
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
        Transfer::request_with_map_params(self.verbose, &node_address, self.rpc_id, params)
    }

    /// Attempt to read a previously-saved deploy from file, and send that to the node.
    pub fn send_deploy_file(self, node_address: &str, input_path: &str) -> Result<JsonRpc> {
        let deploy = Deploy::read_deploy(&input_path)?;
        let params = PutDeployParams { deploy };
        SendDeploy::request_with_map_params(self.verbose, node_address, self.rpc_id, params)
    }

    /// Put a deploy to a node.
    pub fn put_deploy(self, node_address: String, deploy: Deploy) -> Result<JsonRpc> {
        let params = PutDeployParams { deploy };
        PutDeploy::request_with_map_params(self.verbose, &node_address, self.rpc_id, params)
    }

    /// List deploys for the most recent block, or optionally passed block hash.
    pub fn list_deploys(
        self,
        node_address: String,
        maybe_block_hash: Option<BlockHash>,
    ) -> Result<JsonRpc> {
        match maybe_block_hash {
            Some(block_hash) => {
                let params = GetBlockParams { block_hash };
                ListDeploys::request_with_map_params(
                    self.verbose,
                    &node_address,
                    self.rpc_id,
                    params,
                )
            }
            None => ListDeploys::request(self.verbose, &node_address, self.rpc_id),
        }
    }

    /// Get a block from the node.
    pub fn get_block(
        self,
        node_address: &str,
        maybe_block_hash: Option<BlockHash>,
    ) -> Result<JsonRpc> {
        match maybe_block_hash {
            Some(block_hash) => {
                let params = GetBlockParams { block_hash };
                GetBlock::request_with_map_params(self.verbose, &node_address, self.rpc_id, params)
            }
            None => GetBlock::request(self.verbose, &node_address, self.rpc_id),
        }
    }
}

/// General purpose client trait for making requests to casper-node's HTTP endpoints.
pub(crate) trait RpcClient {
    const RPC_METHOD: &'static str;

    /// Call a casper-node JSON-RPC endpoint.
    fn request(verbose: bool, node_address: &str, rpc_id: u32) -> Result<JsonRpc> {
        executor::block_on(async {
            request(
                verbose,
                node_address,
                rpc_id,
                Self::RPC_METHOD,
                Params::None(()),
            )
            .await
        })
    }

    /// Call a casper node JSON-RPC endpoint with parameters.
    fn request_with_map_params<T: IntoJsonMap>(
        verbose: bool,
        node_address: &str,
        rpc_id: u32,
        params: T,
    ) -> Result<JsonRpc> {
        executor::block_on(async {
            request(
                verbose,
                node_address,
                rpc_id,
                Self::RPC_METHOD,
                Params::from(params.into_json_map()),
            )
            .await
        })
    }
}

// TODO - If/when https://github.com/AtsukiTak/warp-json-rpc/pull/1 is merged and published,
//        change `rpc_id` to a `jsonrpc_lite::Id`.
async fn request(
    verbose: bool,
    node_address: &str,
    rpc_id: u32,
    method: &str,
    params: Params,
) -> Result<JsonRpc> {
    let url = format!("{}/{}", node_address, RPC_API_PATH);
    let rpc_req = JsonRpc::request_with_params(rpc_id as i64, method, params);

    if verbose {
        println!(
            "{}",
            serde_json::to_string_pretty(&rpc_req).expect("should encode to JSON")
        );
    }

    let client = Client::new();
    let response = client
        .post(&url)
        .json(&rpc_req)
        .send()
        .await
        .map_err(Error::FailedToGetResponse)?;

    if let Err(error) = response.error_for_status_ref() {
        if verbose {
            println!("Failed Sending {}", error);
        }
        return Err(Error::FailedSending(rpc_req));
    }

    let rpc_response = response.json().await.map_err(Error::FailedToParseResponse);

    if let Err(error) = rpc_response {
        if verbose {
            println!("Failed parsing as a JSON-RPC response: {}", error);
        }
        return Err(error);
    }

    let rpc_response: JsonRpc = rpc_response?;

    if rpc_response.get_result().is_some() {
        if verbose {
            println!("Received successful response:");
        }
        return Ok(rpc_response);
    }

    if let Some(error) = rpc_response.get_error() {
        if verbose {
            println!("Response returned an error");
        }
        return Err(Error::ResponseIsError(error.clone()));
    }

    if verbose {
        println!("Invalid response returned");
    }
    Err(Error::InvalidResponse(rpc_response))
}

pub trait IntoJsonMap: Serialize {
    fn into_json_map(self) -> Map<String, Value>
    where
        Self: Sized,
    {
        json!(self)
            .as_object()
            .unwrap_or_else(|| panic!("should be a JSON object"))
            .clone()
    }
}

impl IntoJsonMap for PutDeployParams {}
impl IntoJsonMap for GetBlockParams {}
impl IntoJsonMap for GetGlobalStateHashParams {}
impl IntoJsonMap for GetDeployParams {}
impl IntoJsonMap for GetBalanceParams {}
impl IntoJsonMap for GetItemParams {}
