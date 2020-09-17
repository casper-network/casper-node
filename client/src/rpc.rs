use std::str;

use futures::executor;
use jsonrpc_lite::{Id as RpcId, JsonRpc, Params};
use reqwest::Client;
use serde_json::{Map, Value};

use crate::{Error, Result};

// CEP/0009 - /rpc endpoints
const RPC_API_PATH: &str = "rpc";

// magic number for JSON-RPC calls
const RPC_ID: RpcId = RpcId::Num(1234);

pub trait RpcClient {
    const RPC_METHOD: &'static str;

    fn request(node_address: &str) -> Result<Value> {
        executor::block_on(async {
            request(node_address, RPC_ID, Self::RPC_METHOD, Params::None(())).await
        })
    }

    fn request_with_array_params(node_address: &str, params: Vec<Value>) -> Result<Value> {
        executor::block_on(async {
            request(node_address, RPC_ID, Self::RPC_METHOD, params.into()).await
        })
    }

    fn request_with_map_params(node_address: &str, params: Map<String, Value>) -> Result<Value> {
        executor::block_on(async {
            request(node_address, RPC_ID, Self::RPC_METHOD, params.into()).await
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
