use futures::executor;
use std::str;

// CEP/0009 - /rpc endpoints
const RPC_API_PATH: &str = "rpc";

// magic number for JSON-RPC calls
const RPC_ID: jsonrpc_lite::Id = jsonrpc_lite::Id::Num(1234);

#[derive(thiserror::Error, Debug)]
pub enum RpcError {
    #[error("Incorrect rpc response {0}")]
    IncorrectResponse(String),
}

pub trait RpcClient {
    const RPC_METHOD: &'static str;

    // TODO(dwerner) async-trait
    fn request_sync<P>(node_address: &str, params: P) -> anyhow::Result<serde_json::Value>
    where
        P: serde::Serialize,
    {
        executor::block_on(async {
            Ok(request(node_address, RPC_ID, Self::RPC_METHOD, params).await?)
        })
    }
}

async fn request<P>(
    node_address: &str,
    id: jsonrpc_lite::Id,
    method: &str,
    params: P,
) -> anyhow::Result<serde_json::Value>
where
    P: serde::Serialize,
{
    let url = format!("{}/{}", node_address, RPC_API_PATH);
    let rpc_req = jsonrpc_lite::JsonRpc::request_with_params(id, method, serde_json::json!(params));
    let client = reqwest::Client::new();
    let body = client
        .post(&url)
        .json(&rpc_req)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("should get response from node: {}", error))?
        .bytes()
        .await
        .map_err(|error| anyhow::anyhow!("should get bytes from node response: {}", error))?;

    if body.is_empty() {
        return Err(RpcError::IncorrectResponse("empty response".to_string()).into());
    }
    let json_encoded = str::from_utf8(body.as_ref())
        .map_err(|e| RpcError::IncorrectResponse(format!("Utf8 error {:?}", e)))?;

    println!("{}", json_encoded);
    let rpc_res = jsonrpc_lite::JsonRpc::parse(&json_encoded)?;
    let res = match rpc_res {
        success @ jsonrpc_lite::JsonRpc::Success(_) => success
            .get_result()
            .ok_or_else(|| RpcError::IncorrectResponse(json_encoded.to_string()))?
            .clone(),
        _ => return Err(RpcError::IncorrectResponse(json_encoded.to_string()).into()),
    };

    Ok(res)
}
