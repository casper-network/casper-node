use std::{process, str};

use casper_node::rpcs::{
    account::PutDeployParams,
    chain::{GetBlockParams, GetStateRootHashParams},
    info::GetDeployParams,
    state::{GetBalanceParams, GetItemParams},
    RPC_API_PATH,
};

use futures::executor;
use jsonrpc_lite::{JsonRpc, Params};
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Map, Value};

pub(crate) trait RpcClient {
    const RPC_METHOD: &'static str;

    fn request(verbose: bool, node_address: &str, rpc_id: u32) -> JsonRpc {
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

    fn request_with_map_params<T: IntoJsonMap>(
        verbose: bool,
        node_address: &str,
        rpc_id: u32,
        params: T,
    ) -> JsonRpc {
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
) -> JsonRpc {
    let url = format!("{}/{}", node_address, RPC_API_PATH);
    let rpc_req = JsonRpc::request_with_params(rpc_id as i64, method, params);

    if verbose {
        println!(
            "Sending request to {}:\n{}\n",
            url,
            serde_json::to_string_pretty(&rpc_req).expect("should encode to JSON")
        );
    }

    let client = Client::new();
    let response = match client.post(&url).json(&rpc_req).send().await {
        Ok(response) => response,
        Err(error) => {
            println!("Failed to get a response: {}", error);
            process::exit(1);
        }
    };

    if let Err(error) = response.error_for_status_ref() {
        println!("Failed sending {:?}: {}", rpc_req, error);
        process::exit(1);
    }

    let rpc_response: JsonRpc = match response.json().await {
        Ok(rpc_response) => rpc_response,
        Err(error) => {
            println!("Failed parsing as a JSON-RPC response: {}", error);
            process::exit(1);
        }
    };

    if rpc_response.get_result().is_some() {
        if verbose {
            println!("Received successful response:");
        }
        return rpc_response;
    }

    if rpc_response.get_error().is_some() {
        println!("Response returned an error");
        println!(
            "{}",
            serde_json::to_string_pretty(&rpc_response).expect("should encode to JSON")
        );
        process::exit(1);
    }

    println!("Invalid response returned");
    println!(
        "{}",
        serde_json::to_string_pretty(&rpc_response).expect("should encode to JSON")
    );
    process::exit(1);
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
