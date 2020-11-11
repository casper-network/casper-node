use futures::executor;
use jsonrpc_lite::{Id, JsonRpc, Params};
use rand::Rng;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Map, Value};

use casper_execution_engine::core::engine_state::ExecutableDeployItem;
use casper_node::{
    crypto::{asymmetric_key::PublicKey, hash::Digest},
    rpcs::{
        account::{PutDeploy, PutDeployParams},
        chain::{
            BlockIdentifier, GetBlock, GetBlockParams, GetStateRootHash, GetStateRootHashParams,
        },
        info::{GetDeploy, GetDeployParams},
        state::{GetAuctionInfo, GetBalance, GetBalanceParams, GetItem, GetItemParams},
        RpcWithOptionalParams, RpcWithParams, RpcWithoutParams, RPC_API_PATH,
    },
    types::{BlockHash, Deploy, DeployHash},
};
use casper_types::{bytesrepr::ToBytes, Key, RuntimeArgs, URef, U512};

use crate::{
    deploy::{DeployExt, DeployParams, SendDeploy, Transfer},
    error::{Error, Result},
    validation,
};

/// Target for a given transfer.
pub(crate) enum TransferTarget {
    /// Transfer to another purse within an account.
    OwnPurse(URef),
    /// Transfer to another account.
    Account(PublicKey),
}

/// Struct representing a single JSON-RPC call to the casper node.
#[derive(Debug)]
pub(crate) struct RpcCall {
    rpc_id: Id,
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
    pub(crate) fn new(maybe_rpc_id: &str, node_address: &str, verbose: bool) -> Result<Self> {
        let rpc_id = if maybe_rpc_id.is_empty() {
            Id::from(rand::thread_rng().gen::<i64>())
        } else if let Ok(i64_id) = maybe_rpc_id.parse::<i64>() {
            Id::from(i64_id)
        } else {
            Id::from(maybe_rpc_id.to_string())
        };

        Ok(Self {
            rpc_id,
            node_address: node_address.trim_end_matches('/').to_string(),
            verbose,
        })
    }

    pub(crate) fn get_deploy(self, deploy_hash: &str) -> Result<JsonRpc> {
        let hash = Digest::from_hex(deploy_hash)?;
        let params = GetDeployParams {
            deploy_hash: DeployHash::new(hash),
        };
        GetDeploy::request_with_map_params(self, params)
    }

    pub(crate) fn get_item(self, state_root_hash: &str, key: &str, path: &str) -> Result<JsonRpc> {
        let state_root_hash = Digest::from_hex(state_root_hash)?;

        let key = {
            if let Ok(key) = Key::from_formatted_str(key) {
                key
            } else if let Ok(public_key) = PublicKey::from_hex(key) {
                Key::Account(public_key.to_account_hash())
            } else {
                return Err(Error::FailedToParseKey);
            }
        };

        let path = if path.is_empty() {
            vec![]
        } else {
            path.split('/').map(ToString::to_string).collect()
        };

        let params = GetItemParams {
            state_root_hash,
            key: key.to_formatted_string(),
            path: path.clone(),
        };
        let response = GetItem::request_with_map_params(self, params)?;
        validation::validate_query_response(&response, &state_root_hash, &key, &path)?;
        Ok(response)
    }

    pub(crate) fn get_state_root_hash(self, maybe_block_identifier: &str) -> Result<JsonRpc> {
        match Self::block_identifier(maybe_block_identifier)? {
            Some(block_identifier) => {
                let params = GetStateRootHashParams { block_identifier };
                GetStateRootHash::request_with_map_params(self, params)
            }
            None => GetStateRootHash::request(self),
        }
    }

    pub(crate) fn get_balance(self, state_root_hash: &str, purse_uref: &str) -> Result<JsonRpc> {
        let state_root_hash = Digest::from_hex(state_root_hash)?;
        let uref = URef::from_formatted_str(purse_uref)
            .map_err(|error| Error::FailedToParseURef("purse_uref", error))?;
        let key = Key::from(uref);

        let params = GetBalanceParams {
            state_root_hash,
            purse_uref: purse_uref.to_string(),
        };
        let response = GetBalance::request_with_map_params(self, params)?;
        validation::validate_get_balance_response(&response, &state_root_hash, &key)?;
        Ok(response)
    }

    pub(crate) fn get_auction_info(self) -> Result<JsonRpc> {
        GetAuctionInfo::request(self)
    }

    pub(crate) fn transfer(
        self,
        amount: U512,
        source_purse: Option<URef>,
        target: TransferTarget,
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
        match target {
            TransferTarget::Account(target_account) => {
                let target_account_hash = target_account.to_account_hash().value();
                transfer_args.insert(TRANSFER_ARG_TARGET, target_account_hash);
            }
            TransferTarget::OwnPurse(target_purse) => {
                transfer_args.insert(TRANSFER_ARG_TARGET, target_purse);
            }
        }
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args.to_bytes()?,
        };
        let deploy = Deploy::with_payment_and_session(deploy_params, payment, session);
        let params = PutDeployParams { deploy };
        Transfer::request_with_map_params(self, params)
    }

    pub(crate) fn send_deploy_file(self, input_path: &str) -> Result<JsonRpc> {
        let deploy = Deploy::read_deploy(input_path)?;
        let params = PutDeployParams { deploy };
        SendDeploy::request_with_map_params(self, params)
    }

    pub(crate) fn put_deploy(self, deploy: Deploy) -> Result<JsonRpc> {
        let params = PutDeployParams { deploy };
        PutDeploy::request_with_map_params(self, params)
    }

    pub(crate) fn get_block(self, maybe_block_identifier: &str) -> Result<JsonRpc> {
        let maybe_block_identifier = Self::block_identifier(maybe_block_identifier)?;
        let response = match maybe_block_identifier {
            Some(block_identifier) => {
                let params = GetBlockParams { block_identifier };
                GetBlock::request_with_map_params(self, params)
            }
            None => GetBlock::request(self),
        }?;
        validation::validate_get_block_response(&response, &maybe_block_identifier)?;
        Ok(response)
    }

    fn block_identifier(maybe_block_identifier: &str) -> Result<Option<BlockIdentifier>> {
        if maybe_block_identifier.is_empty() {
            return Ok(None);
        }

        if maybe_block_identifier.len() == (Digest::LENGTH * 2) {
            let hash = Digest::from_hex(maybe_block_identifier)?;
            Ok(Some(BlockIdentifier::Hash(BlockHash::new(hash))))
        } else {
            let height = maybe_block_identifier
                .parse()
                .map_err(|error| Error::FailedToParseInt("block_identifier", error))?;
            Ok(Some(BlockIdentifier::Height(height)))
        }
    }

    async fn request(self, method: &str, params: Params) -> Result<JsonRpc> {
        let url = format!("{}/{}", self.node_address, RPC_API_PATH);
        let rpc_req = JsonRpc::request_with_params(self.rpc_id, method, params);

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
        Err(Error::InvalidRpcResponse(rpc_response))
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

impl RpcClient for GetBalance {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl RpcClient for GetBlock {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl RpcClient for GetStateRootHash {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl RpcClient for GetItem {
    const RPC_METHOD: &'static str = <Self as RpcWithParams>::METHOD;
}

impl RpcClient for GetAuctionInfo {
    const RPC_METHOD: &'static str = Self::METHOD;
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
