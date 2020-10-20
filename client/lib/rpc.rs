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
        chain::{GetBlock, GetBlockParams, GetStateRootHash, GetStateRootHashParams},
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

/// Struct representing a single JSON-RPC call to the casper node.
#[derive(Debug, Default)]
pub struct RpcCall {
    // TODO - If/when https://github.com/AtsukiTak/warp-json-rpc/pull/1 is merged and published,
    //        change `rpc_id` to a `jsonrpc_lite::Id`.
    rpc_id: u32,
    node_address: String,
    verbose: bool,
}

/// `RpcCall` encapsulates calls made to the casper node service via JSON-RPC.
impl RpcCall {
    /// Creates a new RPC instance.
    ///
    /// `rpc_id` is used for RPC-ID as required by the JSON-RPC specification, and is returned by
    /// the node in the corresponding response.
    ///
    /// `node_address` identifies the network address of the target node's HTTP server, e.g.
    /// `"http://127.0.0.1:7777"`.
    ///
    /// When `verbose` is `true`, the request will be printed to `stdout`.
    pub fn new(rpc_id: u32, node_address: String, verbose: bool) -> Self {
        Self {
            rpc_id,
            node_address,
            verbose,
        }
    }

    /// Gets a `Deploy` from the node.
    pub fn get_deploy(self, deploy_hash: DeployHash) -> Result<JsonRpc> {
        let params = GetDeployParams { deploy_hash };
        GetDeploy::request_with_map_params(self, params)
    }

    /// Queries the node for an item at `key`, given a `path` and a `state_root_hash`.
    pub fn get_item(
        self,
        state_root_hash: Digest,
        key: String,
        path: Vec<String>,
    ) -> Result<JsonRpc> {
        let params = GetItemParams {
            state_root_hash,
            key,
            path,
        };
        GetItem::request_with_map_params(self, params)
    }

    /// Queries the node for the most recent state root hash or as of a given block hash if
    /// `maybe_block_hash` is provided.
    pub fn get_state_root_hash(self, maybe_block_hash: Option<BlockHash>) -> Result<JsonRpc> {
        match maybe_block_hash {
            Some(block_hash) => {
                let params = GetStateRootHashParams { block_hash };
                GetStateRootHash::request_with_map_params(self, params)
            }
            None => GetStateRootHash::request(self),
        }
    }

    /// Gets the balance from a purse.
    pub fn get_balance(self, state_root_hash: Digest, purse_uref: String) -> Result<JsonRpc> {
        let params = GetBalanceParams {
            state_root_hash,
            purse_uref,
        };
        GetBalance::request_with_map_params(self, params)
    }

    /// Transfers an amount between purses.
    pub fn transfer(
        self,
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
        Transfer::request_with_map_params(self, params)
    }

    /// Reads a previously-saved `Deploy` from file, and sends that to the node.
    pub fn send_deploy_file(self, input_path: &str) -> Result<JsonRpc> {
        let deploy = Deploy::read_deploy(input_path)?;
        let params = PutDeployParams { deploy };
        SendDeploy::request_with_map_params(self, params)
    }

    /// Puts a `Deploy` to the node.
    pub fn put_deploy(self, deploy: Deploy) -> Result<JsonRpc> {
        let params = PutDeployParams { deploy };
        PutDeploy::request_with_map_params(self, params)
    }

    /// List `Deploy`s included in the specified `Block`.
    ///
    /// If `maybe_block_hash` is `Some`, that specific block is used, otherwise the most
    /// recently-added block is used.
    pub fn list_deploys(self, maybe_block_hash: Option<BlockHash>) -> Result<JsonRpc> {
        match maybe_block_hash {
            Some(block_hash) => {
                let params = GetBlockParams { block_hash };
                ListDeploys::request_with_map_params(self, params)
            }
            None => ListDeploys::request(self),
        }
    }

    /// Gets a `Block` from the node.
    ///
    /// If `maybe_block_hash` is `Some`, that specific `Block` is retrieved, otherwise the most
    /// recently-added `Block` is retrieved.
    pub fn get_block(self, maybe_block_hash: Option<BlockHash>) -> Result<JsonRpc> {
        match maybe_block_hash {
            Some(block_hash) => {
                let params = GetBlockParams { block_hash };
                GetBlock::request_with_map_params(self, params)
            }
            None => GetBlock::request(self),
        }
    }

    async fn request(self, method: &str, params: Params) -> Result<JsonRpc> {
        let url = format!("{}/{}", self.node_address, RPC_API_PATH);
        let rpc_req = JsonRpc::request_with_params(self.rpc_id as i64, method, params);

        if self.verbose {
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
            if self.verbose {
                println!("Failed Sending {}", error);
            }
            return Err(Error::FailedSending(rpc_req));
        }

        let rpc_response = response.json().await.map_err(Error::FailedToParseResponse);

        if let Err(error) = rpc_response {
            if self.verbose {
                println!("Failed parsing as a JSON-RPC response: {}", error);
            }
            return Err(error);
        }

        let rpc_response: JsonRpc = rpc_response?;

        if rpc_response.get_result().is_some() {
            if self.verbose {
                println!("Received successful response:");
            }
            return Ok(rpc_response);
        }

        if let Some(error) = rpc_response.get_error() {
            if self.verbose {
                println!("Response returned an error");
            }
            return Err(Error::ResponseIsError(error.clone()));
        }

        if self.verbose {
            println!("Invalid response returned");
        }
        Err(Error::InvalidResponse(rpc_response))
    }
}

/// General purpose client trait for making requests to casper node's HTTP endpoints.
pub(crate) trait RpcClient {
    const RPC_METHOD: &'static str;

    /// Calls a casper node's JSON-RPC endpoint.
    fn request(rpc_call: RpcCall) -> Result<JsonRpc> {
        executor::block_on(async { rpc_call.request(Self::RPC_METHOD, Params::None(())).await })
    }

    /// Calls a casper node's JSON-RPC endpoint with parameters.
    fn request_with_map_params<T: IntoJsonMap>(rpc_call: RpcCall, params: T) -> Result<JsonRpc> {
        executor::block_on(async {
            rpc_call
                .request(Self::RPC_METHOD, Params::from(params.into_json_map()))
                .await
        })
    }
}

pub(crate) trait IntoJsonMap: Serialize {
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
impl IntoJsonMap for GetStateRootHashParams {}
impl IntoJsonMap for GetDeployParams {}
impl IntoJsonMap for GetBalanceParams {}
impl IntoJsonMap for GetItemParams {}
