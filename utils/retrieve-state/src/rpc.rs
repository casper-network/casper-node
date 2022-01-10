use jsonrpc_lite::{JsonRpc, Params};
use reqwest::Client;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;

/// Represents errors that can happen during calls to the rpc function.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Error in the reqwest crate.
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    /// Error in the JSON-RPC crate.
    #[error(transparent)]
    JsonRpc(#[from] jsonrpc_lite::Error),
    /// Error deserializing the response.
    #[error(transparent)]
    DeserializeError(#[from] serde_json::Error),
}

pub(crate) async fn rpc<'de, R, P>(
    client: &Client,
    url: &str,
    method: &str,
    params: P,
) -> Result<R, Error>
where
    R: DeserializeOwned,
    P: Serialize,
{
    let params = Params::from(json!(params));
    let rpc_req = JsonRpc::request_with_params(12345, method, params);
    let response = client.post(url).json(&rpc_req).send().await?;
    let rpc_res: JsonRpc = response.json().await?;
    if let Some(error) = rpc_res.get_error() {
        return Err(error.clone().into());
    }
    let value = rpc_res.get_result().unwrap();
    let deserialized = serde_json::from_value(value.clone())?;
    Ok(deserialized)
}

pub(crate) async fn rpc_without_params<'de, R>(
    client: &Client,
    url: &str,
    method: &str,
) -> Result<R, Error>
where
    R: DeserializeOwned,
{
    let rpc_req = JsonRpc::request(12345, method);
    let response = client.post(url).json(&rpc_req).send().await?;
    let rpc_res: JsonRpc = response.json().await?;
    if let Some(error) = rpc_res.get_error() {
        println!("res {:?}", rpc_res);
        return Err(error.clone().into());
    }
    let value = rpc_res.get_result().unwrap();
    let deserialized = serde_json::from_value(value.clone())?;
    Ok(deserialized)
}
