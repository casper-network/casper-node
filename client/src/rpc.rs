use std::str;

use casper_node::rpcs::{
    account::PutDeployParams,
    chain::{GetBlockParams, GetGlobalStateHashParams},
    info::{GetDeployParams, GetBlockParams as BlockParams},
    state::{GetBalanceParams, GetItemParams},
    RPC_API_PATH,
};

use futures::executor;
use jsonrpc_lite::{Id as RpcId, JsonRpc, Params};
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::{Error, Result};

// Magic number for JSON-RPC calls.
const RPC_ID: RpcId = RpcId::Num(1234);

pub(crate) trait RpcClient {
    const RPC_METHOD: &'static str;

    fn request(node_address: &str) -> Result<Value> {
        executor::block_on(async {
            request(node_address, RPC_ID, Self::RPC_METHOD, Params::None(())).await
        })
    }

    fn request_with_map_params<T: IntoJsonMap>(node_address: &str, params: T) -> Result<Value> {
        executor::block_on(async {
            request(
                node_address,
                RPC_ID,
                Self::RPC_METHOD,
                Params::from(params.into_json_map()),
            )
            .await
        })
    }
}

async fn request(node_address: &str, id: RpcId, method: &str, params: Params) -> Result<Value> {
    let url = format!("{}/{}", node_address, RPC_API_PATH);
    let rpc_req = JsonRpc::request_with_params(id, method, params);
    let client = Client::new();
    let response = client
        .post(&url)
        .json(&rpc_req)
        .send()
        .await
        .map_err(Error::FailedToGetResponse)?;

    if let Err(error) = response.error_for_status_ref() {
        panic!("Failed sending {:?}: {}", rpc_req, error);
    }

    let rpc_response: JsonRpc = response
        .json()
        .await
        .map_err(Error::FailedToParseResponse)?;

    if let Some(success) = rpc_response.get_result() {
        return Ok(success.clone());
    }

    if let Some(error) = rpc_response.get_error() {
        return Err(Error::ResponseIsError(error.clone()));
    }

    Err(Error::InvalidResponse(rpc_response))
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
impl IntoJsonMap for GetGlobalStateHashParams {}
impl IntoJsonMap for GetDeployParams {}
impl IntoJsonMap for BlockParams {}
impl IntoJsonMap for GetBalanceParams {}
impl IntoJsonMap for GetItemParams {}
