//! RPCs related to finding information about currently supported RPCs.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use casper_types::U512;
use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use lazy_static::lazy_static;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use warp_json_rpc::Builder;

use super::{Error, state::GetBalance, ReactorEventT, RpcWithOptionalParams, RpcWithParams, RpcWithoutParams, RpcWithoutParamsExt, account::{PutDeploy, PutDeployParams, PutDeployResult}, chain::{
        BlockIdentifier, GetBlock, GetBlockParams, GetBlockResult, GetStateRootHash,
        GetStateRootHashParams, GetStateRootHashResult,
    }, info::{GetDeploy, GetDeployParams, GetDeployResult, GetPeers, GetPeersResult, GetStatus}, state::GetAuctionInfo, state::GetAuctionInfoResult, state::GetBalanceParams, state::GetBalanceResult};
use crate::{components::{chainspec_loader::ChainspecInfo, CLIENT_API_VERSION}, effect::EffectBuilder, types::json_compatibility::AuctionState, crypto::hash::Digest, types::{Block, Deploy, GetStatusResult, NodeId, PeersMap, StatusFeed}};

lazy_static! {
    static ref DOCS_RPC_RESULT: GetRpcsResult = {
        let mut result = GetRpcsResult {
            api_version: CLIENT_API_VERSION.clone(),
            rpcs:  vec![],
        };

        // Setup PutDeploy example. 
           let deploy = Deploy::doc_example();
           let response_result = PutDeployResult {
                api_version: CLIENT_API_VERSION.clone(),
                deploy_hash: *deploy.id()
            };
            let request_params = PutDeployParams {
                deploy: deploy.clone()
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
                block: Some(block.clone())
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
                deploy: deploy.clone(),
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
            peers_hashmap.insert(node_id.clone(), socket_addr);

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
            let chainspec_info = ChainspecInfo::doc_example().clone();
            let status_feed = StatusFeed::<NodeId> {
                last_added_block: Some(Block::doc_example().clone()),
                peers: peers_for_status,
                chainspec_info,
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

            // Setup GetBalance. 
            let request_params = GetBalanceParams {
                state_root_hash: Digest::doc_example(),
                purse_uref: String::from("uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-007")
            };

            let response_result = GetBalanceResult {
                api_version: CLIENT_API_VERSION.clone(),
                balance_value: U512::from(1234567),
                // TODO! Find a concrete example of proof. 
                merkle_proof: String::from("Proof"),
            };

            result.push_with_params::<GetBalance> (
                "Retrieves a purse's balance from the network.",
                None,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/state/struct.GetBalanceParams.html",
                None,
                request_params,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/state/struct.GetBalanceResult.html",
                None,
                response_result,
            );

            // Setup GetAuctionInfo. 
            let auction_info = AuctionState::doc_example().clone();
            let response_result = GetAuctionInfoResult {
                api_version: CLIENT_API_VERSION.clone(),
                auction_state: auction_info
            };

            result.push_without_params::<GetAuctionInfo> (
                "Retrieves the bids and validators as of the most recently added Block",
                None,
                "https://docs.rs/casper-node/latest/casper_node/rpcs/state/struct.GetAuctionInfoResult.html",
                None,
                response_result,
            );

        result
    };
}
/// A trait to generate static hardcode representations of data structures to present for RPC calls.
pub trait DocExample {
    /// Generate a hardcoded, possibly invalid representation of the requested data structure.
    fn doc_example() -> &'static Self;
}

impl DocExample for GetRpcsResult {
    fn doc_example() -> &'static Self {
        &*DOCS_RPC_RESULT
    }
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
            let result = GetRpcsResult::doc_example();
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}
