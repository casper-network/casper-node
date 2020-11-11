//! RPCs related to finding information about currently supported RPCs.

use std::{collections::{HashMap}, net::{IpAddr, Ipv4Addr, SocketAddr}};

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use lazy_static::lazy_static;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use warp_json_rpc::Builder;

use super::{Error, ReactorEventT, RpcWithOptionalParams, RpcWithParams, RpcWithoutParams, RpcWithoutParamsExt, account::{PutDeploy, PutDeployParams, PutDeployResult}, chain::{
        BlockIdentifier, GetBlock, GetBlockParams, GetBlockResult, GetStateRootHash,
        GetStateRootHashParams, GetStateRootHashResult,
    }, info::GetStatus, info::{GetDeploy, GetDeployParams, GetDeployResult, GetPeers, GetPeersResult}};
use crate::{components::{CLIENT_API_VERSION, chainspec_loader::ChainspecInfo}, effect::EffectBuilder, types::{Block, Deploy, GetStatusResult, NodeId, PeersMap, StatusFeed}};

use casper_types::bytesrepr::FromBytes;

lazy_static! {
    static ref DEPLOY_BYTES: Vec<u8> = vec![
        2, 33, 0, 0, 0, 3, 194, 2, 147, 167, 80, 25, 150, 126, 210, 95, 197, 200, 155, 136, 58,
        251, 207, 109, 87, 4, 58, 5, 55, 52, 133, 174, 163, 89, 73, 165, 174, 214, 238, 89, 104,
        179, 117, 1, 0, 0, 73, 203, 21, 0, 0, 0, 0, 0, 66, 0, 0, 0, 0, 0, 0, 0, 184, 111, 11, 218,
        103, 81, 155, 251, 84, 17, 21, 104, 10, 111, 184, 149, 217, 228, 207, 244, 89, 125, 161,
        179, 89, 68, 0, 136, 4, 10, 66, 245, 3, 0, 0, 0, 0, 221, 195, 95, 161, 210, 98, 101, 41,
        150, 69, 99, 239, 39, 107, 153, 30, 233, 145, 208, 38, 85, 43, 0, 117, 63, 108, 91, 191,
        54, 15, 168, 93, 76, 161, 91, 164, 18, 144, 142, 230, 83, 79, 126, 202, 40, 3, 46, 135,
        209, 130, 16, 207, 126, 52, 83, 85, 119, 217, 26, 114, 84, 65, 181, 127, 101, 58, 90, 126,
        218, 165, 210, 35, 37, 23, 86, 131, 157, 214, 156, 32, 82, 33, 32, 85, 105, 104, 161, 67,
        174, 255, 116, 222, 25, 0, 137, 14, 0, 0, 0, 99, 97, 115, 112, 101, 114, 45, 101, 120, 97,
        109, 112, 108, 101, 149, 12, 122, 40, 178, 90, 55, 251, 131, 194, 207, 248, 81, 166, 229,
        99, 149, 142, 153, 203, 251, 74, 65, 137, 117, 22, 200, 221, 160, 93, 250, 230, 5, 14, 0,
        0, 0, 200, 225, 125, 28, 20, 197, 81, 14, 201, 111, 255, 63, 176, 126, 0, 81, 0, 0, 0, 62,
        52, 123, 252, 248, 153, 62, 203, 67, 47, 96, 67, 226, 110, 224, 173, 178, 1, 101, 153, 174,
        203, 249, 91, 114, 178, 106, 41, 19, 108, 159, 228, 142, 120, 17, 171, 148, 187, 84, 194,
        31, 222, 200, 52, 50, 63, 178, 221, 197, 20, 100, 251, 8, 19, 168, 97, 254, 81, 99, 145,
        207, 211, 52, 67, 205, 141, 215, 207, 247, 66, 39, 22, 27, 201, 106, 154, 18, 186, 75, 47,
        234, 46, 0, 0, 0, 227, 55, 119, 100, 99, 177, 78, 23, 153, 57, 28, 30, 186, 16, 212, 231,
        110, 239, 117, 220, 116, 2, 157, 226, 220, 227, 148, 48, 141, 117, 134, 23, 19, 104, 169,
        64, 146, 214, 255, 241, 67, 106, 26, 89, 150, 212, 1, 0, 0, 0, 2, 33, 0, 0, 0, 3, 194, 2,
        147, 167, 80, 25, 150, 126, 210, 95, 197, 200, 155, 136, 58, 251, 207, 109, 87, 4, 58, 5,
        55, 52, 133, 174, 163, 89, 73, 165, 174, 214, 2, 64, 0, 0, 0, 60, 152, 32, 42, 96, 198, 19,
        131, 2, 61, 89, 43, 12, 5, 116, 70, 45, 228, 207, 174, 205, 211, 193, 63, 20, 138, 219, 77,
        218, 62, 226, 65, 93, 162, 107, 117, 154, 213, 27, 227, 102, 245, 175, 196, 230, 141, 120,
        198, 255, 250, 94, 158, 103, 60, 76, 245, 150, 22, 96, 36, 69, 43, 118, 146
    ];
    static ref DEPLOY: Deploy = FromBytes::from_bytes(&DEPLOY_BYTES).unwrap().0;
}
/// A trait to generate static hardcode representations of data structures to present for RPC calls.
pub trait DocExample {
    /// Generate a hardcoded, possibly invalid representation of the requested data structure.
    fn doc_example() -> Self;
}

#[derive(Debug, Serialize, Deserialize)]
struct RequestParams {
    requirement: String,
    docs_url: Option<String>,
    notes: Option<String>,
    example: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResponseResult {
    docs_url: Option<String>,
    notes: Option<String>,
    example: Value,
}

#[derive(Debug, Serialize, Deserialize)]
/// The struct containing the documentation for the RPCs.
pub struct RpcDocs {
    method: String,
    summary: String,
    notes: Option<String>,
    request_params: Option<RequestParams>,
    response_result: ResponseResult,
}

/// Result for "docs_get_rpcs" RPC response.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRpcsResult {
    /// The RPC API Version.
    api_version: Version,
    /// The List of supported RPCs.
    rpcs: Vec<RpcDocs>,
}

impl GetRpcsResult {
    #[allow(clippy::too_many_arguments)]
    fn push_with_params<T: RpcWithParams>(
        &mut self,
        summary: &str,
        notes: Option<&str>,
        request_params_docs_url: &str,
        request_params_notes: Option<&str>,
        request_params_example: T::RequestParams,
        response_result_docs_url: &str,
        response_result_notes: Option<&str>,
        response_result_example: T::ResponseResult,
    ) {
        let request_params = RequestParams {
            requirement: String::from("params must be presenet"),
            docs_url: Some(request_params_docs_url.to_string()),
            notes: request_params_notes.map(|string| string.to_string()),
            example: Some(json!(request_params_example)),
        };

        let response_result = ResponseResult {
            docs_url: Some(response_result_docs_url.to_string()),
            notes: response_result_notes.map(|string| string.to_string()),
            example: json!(response_result_example),
        };

        let docs = RpcDocs {
            method: T::METHOD.to_string(),
            summary: summary.to_string(),
            notes: notes.map(|string| string.to_string()),
            request_params: Some(request_params),
            response_result,
        };

        self.rpcs.push(docs);
    }

    #[allow(clippy::too_many_arguments)]
    fn push_with_optional_params<T: RpcWithOptionalParams>(
        &mut self,
        summary: &str,
        notes: Option<&str>,
        request_params_docs_url: &str,
        request_params_notes: Option<&str>,
        request_params_example: T::OptionalRequestParams,
        response_result_docs_url: &str,
        response_result_notes: Option<&str>,
        response_result_example: T::ResponseResult,
    ) {
        let request_params = RequestParams {
            requirement: "params may or may not be neccessary".to_string(),
            docs_url: Some(request_params_docs_url.to_string()),
            notes: request_params_notes.map(|string| string.to_string()),
            example: Some(json!(request_params_example)),
        };

        let response_result = ResponseResult {
            docs_url: Some(response_result_docs_url.to_string()),
            notes: response_result_notes.map(|string| string.to_string()),
            example: json!(response_result_example),
        };

        let docs = RpcDocs {
            method: T::METHOD.to_string(),
            summary: summary.to_string(),
            notes: notes.map(|string| string.to_string()),
            request_params: Some(request_params),
            response_result,
        };

        self.rpcs.push(docs);
    }

    #[allow(dead_code)]
    fn push_without_params<T: RpcWithoutParams>(
        &mut self,
        summary: &str,
        notes: Option<&str>,
        response_result_docs_url: &str,
        response_result_notes: Option<&str>,
        response_result_example: T::ResponseResult,
    ) {
        let response_result = ResponseResult {
            docs_url: Some(response_result_docs_url.to_string()),
            notes: response_result_notes.map(|string| string.to_string()),
            example: json!(response_result_example),
        };

        let docs = RpcDocs {
            method: T::METHOD.to_string(),
            summary: summary.to_string(),
            notes: notes.map(|string| string.to_string()),
            request_params: None,
            response_result,
        };

        self.rpcs.push(docs);
    }
}

/// "docs_get_rpcs" RPC.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRpcs {}

impl RpcWithoutParams for GetRpcs {
    const METHOD: &'static str = "docs_get_rpcs";
    type ResponseResult = GetRpcsResult;
}

impl RpcWithoutParamsExt for GetRpcs {
    fn handle_request<REv: ReactorEventT>(
        _effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // Create the RPC Result to populate. 
            let mut result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                rpcs: vec![]
            };

            // Setup PutDeploy example. 
            let deploy = Deploy::doc_example();
            let response_result = PutDeployResult {
                api_version: CLIENT_API_VERSION.clone(),
                deploy_hash: *deploy.id()
            };
            let request_params = PutDeployParams {
                deploy
            };
            result.push_with_params::<PutDeploy>(
                "Creates a Deploy and sends it to the network for execution.",
                None,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/account/struct.PutDeployParams.html",
                None,
                request_params,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/account/struct.PutDeployResult.html",
                None,
                response_result,
            );

            // Setup GetBlock Example. 

            let block = Block::doc_example();

            let request_params = GetBlockParams {
                block_identifier: BlockIdentifier::Height(0),
            };

            let response_result = GetBlockResult {
                api_version: CLIENT_API_VERSION.clone(),
                block: Some(block)
            };

            result.push_with_optional_params::<GetBlock>(
                "Retrieves a `Block` from the network.",
                None,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/chain/struct.GetBlockParams.html",
                Some(r#"The request params can identify a Block by its height (as shown) or its hash (e.g. "Hash": 3c53f1b1c87d977222c6503832ef8592232c15109144ebbd9354f1eb344c0682"#),
                request_params,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/chain/struct.GetBlockResult.html",
                None,
                response_result,
            );

            // Setup GetStateRootHash example. 
            let block = Block::doc_example();
            let request_params = GetStateRootHashParams {
                block_identifier: BlockIdentifier::Hash(*block.hash()),
            };
            let response_result = GetStateRootHashResult {
                api_version: CLIENT_API_VERSION.clone(),
                state_root_hash: Some(*block.state_root_hash()),
            };
            result.push_with_optional_params::<GetStateRootHash>(
                "Retrieves a state root hash at a given Block",
                None,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/chain/struct.GetStateRootHashParams.html",
                Some(r#"The request params can identify a Block by its hash (as shown) or its height (e.g. "Height": 999"#),
                request_params,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/chain/struct.GetStateRootHashResult.html",
                None,
                response_result,
            );

            // Setup GetDeploy Example. 
            let deploy = Deploy::doc_example();

            let request_params = GetDeployParams {
                deploy_hash: *deploy.id()
            };

            let response_result = GetDeployResult {
                api_version: CLIENT_API_VERSION.clone(),
                deploy,
                execution_results: vec![]
            };

            result.push_with_params::<GetDeploy>(
                "Retrieves a Deploy from the network",
                None,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/info/struct.GetDeployParams.html",
                None,
                request_params,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/info/struct.GetDeployResult.html",
                None,
                response_result,
            );

            // Setup GetPeers.
            let node_id = NodeId::doc_example();
            let socket_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127,0,0,1)),8888);
            let mut peers_hashmap: HashMap<NodeId, SocketAddr> = HashMap::new();
            peers_hashmap.insert(node_id, socket_addr);

            let peers_for_status = peers_hashmap.clone();

            let peers = PeersMap::from(peers_hashmap).into();
            let response_result = GetPeersResult {
                api_version: CLIENT_API_VERSION.clone(),
                peers
            };

            result.push_without_params::<GetPeers>(
                "Get peers connected to this node",
                None,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/info/struct.GetPeersResult.html",
                None,
                response_result,
            );

            // Setup GetStatus.
            let status_feed = StatusFeed::<NodeId> {
                last_added_block: Some(Block::doc_example()),
                peers: peers_for_status,
                chainspec_info: ChainspecInfo::doc_example(),
                version: crate::VERSION_STRING.as_str()
            };

            let mut response_result = GetStatusResult::from(status_feed);
            response_result.set_api_version(CLIENT_API_VERSION.clone());

            result.push_without_params::<GetStatus>(
                "Get the current status of the node",
                None,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/info/struct.GetStatusResult.html",
                None,
                response_result,
            );

            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}
