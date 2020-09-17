use std::str;

use futures::executor;
use jsonrpc_lite::{Id as RpcId, JsonRpc};
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{Error, Result};

// CEP/0009 - /rpc endpoints
const RPC_API_PATH: &str = "rpc";

// magic number for JSON-RPC calls
const RPC_ID: RpcId = RpcId::Num(1234);

pub trait RpcClient {
    const RPC_METHOD: &'static str;

    // TODO(dwerner) async-trait
    fn request_sync<P>(node_address: &str, params: P) -> Result<Value>
    where
        P: Serialize,
    {
        executor::block_on(async { request(node_address, RPC_ID, Self::RPC_METHOD, params).await })
    }
}

async fn request<P>(node_address: &str, id: RpcId, method: &str, params: P) -> Result<Value>
where
    P: Serialize,
{
    let url = format!("{}/{}", node_address, RPC_API_PATH);
    let rpc_req = JsonRpc::request_with_params(id, method, json!(params));
    let client = Client::new();
    let rpc_response: JsonRpc = client
        .post(&url)
        .json(&rpc_req)
        .send()
        .await
        .map_err(Error::FailedToGetResponse)?
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
