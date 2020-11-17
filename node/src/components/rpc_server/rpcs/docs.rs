//! RPCs related to finding information about currently supported RPCs.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use lazy_static::lazy_static;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use warp_json_rpc::Builder;

use casper_types::{CLValue, U512};

use super::{
    account::{PutDeploy, PutDeployParams, PutDeployResult},
    chain::{
        BlockIdentifier, GetBlock, GetBlockParams, GetBlockResult, GetStateRootHash,
        GetStateRootHashParams, GetStateRootHashResult,
    },
    info::{
        GetDeploy, GetDeployParams, GetDeployResult, GetPeers, GetPeersResult, GetStatus,
        JsonExecutionResult,
    },
    state::{
        GetAuctionInfo, GetAuctionInfoResult, GetBalance, GetBalanceParams, GetBalanceResult,
        GetItem, GetItemParams, GetItemResult,
    },
    Error, ReactorEventT, RpcWithOptionalParams, RpcWithParams, RpcWithoutParams,
    RpcWithoutParamsExt,
};
use crate::{
    components::{chainspec_loader::ChainspecInfo, CLIENT_API_VERSION},
    crypto::hash::Digest,
    effect::EffectBuilder,
    types::{
        json_compatibility::{AuctionState, ExecutionResult, StoredValue},
        Block, BlockHash, Deploy, GetStatusResult, NodeId, PeersMap, StatusFeed,
    },
};

lazy_static! {
    static ref MERKLE_PROOF: String = String::from("01000000006ef2e0949ac76e55812421f755abe129b6244fe7168b77f47a72536147614625016ef2e0949ac76e55812421f755abe129b6244fe7168b77f47a72536147614625000000003529cde5c621f857f75f3810611eb4af3f998caaa9d4a3413cf799f99c67db0307010000006ef2e0949ac76e55812421f755abe129b6244fe7168b77f47a7253614761462501010102000000006e06000000000074769d28aac597a36a03a932d4b43e4f10bf0403ee5c41dd035102553f5773631200b9e173e8f05361b681513c14e25e3138639eb03232581db7557c9e8dbbc83ce94500226a9a7fe4f2b7b88d5103a4fc7400f02bf89c860c9ccdd56951a2afe9be0e0267006d820fb5676eb2960e15722f7725f3f8f41030078f8b2e44bf0dc03f71b176d6e800dc5ae9805068c5be6da1a90b2528ee85db0609cc0fb4bd60bbd559f497a98b67f500e1e3e846592f4918234647fca39830b7e1e6ad6f5b7a99b39af823d82ba1873d000003000000010186ff500f287e9b53f823ae1582b1fa429dfede28015125fd233a31ca04d5012002015cc42669a55467a1fdf49750772bfc1aed59b9b085558eb81510e9b015a7c83b0301e3cf4a34b1db6bfa58808b686cb8fe21ebe0c1bcbcee522649d2b135fe510fe3");

    static ref DOCS_RPC_RESULT: GetRpcsResult = {
        let mut result = GetRpcsResult {
            api_version: CLIENT_API_VERSION.clone(),
            rpcs: vec![],
        };

        // Setup PutDeploy example.
        let deploy = Deploy::doc_example();
        let response_result = PutDeployResult {
            api_version: CLIENT_API_VERSION.clone(),
            deploy_hash: *deploy.id(),
        };
        let request_params = PutDeployParams {
            deploy: deploy.clone(),
        };
        result.push_with_params::<PutDeploy>(
            "Creates a Deploy and sends it to the network for execution.",
            "https://docs.rs/casper-node/latest/casper_node/rpcs/account/struct.PutDeployParams.html",
            request_params,
            "https://docs.rs/casper-node/latest/casper_node/rpcs/account/struct.PutDeployResult.html",
            response_result,
        );

        // Setup GetBlock Example.
        let block = Block::doc_example();

        let request_params = GetBlockParams {
            block_identifier: BlockIdentifier::Height(0),
        };

        let response_result = GetBlockResult {
            api_version: CLIENT_API_VERSION.clone(),
            block: Some(block.clone()),
        };

        result.push_with_optional_params::<GetBlock>(
                    "Retrieves a Block from the network.",
                    "https://docs.rs/casper-node/latest/casper_node/rpcs/chain/struct.GetBlockParams.html",
                    request_params,
                    "https://docs.rs/casper-node/latest/casper_node/rpcs/chain/struct.GetBlockResult.html",
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
                    "Retrieves a state root hash at a given Block.",
                    "https://docs.rs/casper-node/latest/casper_node/rpcs/chain/struct.GetStateRootHashParams.html",
                    request_params,
                    "https://docs.rs/casper-node/latest/casper_node/rpcs/chain/struct.GetStateRootHashResult.html",
                    response_result,
                );

        // Setup GetDeploy Example.
        let deploy = Deploy::doc_example();

        let request_params = GetDeployParams {
            deploy_hash: *deploy.id(),
        };

        let execution_results = JsonExecutionResult {
            block_hash: BlockHash::new(Digest::from([3u8; Digest::LENGTH])),
            result: ExecutionResult::doc_example().clone(),
        };

        let response_result = GetDeployResult {
            api_version: CLIENT_API_VERSION.clone(),
            deploy: deploy.clone(),
            execution_results: vec![execution_results],
        };

        result.push_with_params::<GetDeploy>(
            "Retrieves a Deploy from the network.",
            "https://docs.rs/casper-node/latest/casper_node/rpcs/info/struct.GetDeployParams.html",
            request_params,
            "https://docs.rs/casper-node/latest/casper_node/rpcs/info/struct.GetDeployResult.html",
            response_result,
        );

        // Setup GetPeers.
        let node_id = NodeId::doc_example();
        let socket_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8888);
        let mut peers_hashmap: HashMap<NodeId, SocketAddr> = HashMap::new();
        peers_hashmap.insert(node_id.clone(), socket_addr);

        let peers_for_status = peers_hashmap.clone();

        let peers = PeersMap::from(peers_hashmap).into();
        let response_result = GetPeersResult {
            api_version: CLIENT_API_VERSION.clone(),
            peers,
        };

        result.push_without_params::<GetPeers>(
            "Retrieves a list of peers connected to this node.",
            "https://docs.rs/casper-node/latest/casper_node/rpcs/info/struct.GetPeersResult.html",
            response_result,
        );

        // Setup GetStatus.
        let chainspec_info = ChainspecInfo::doc_example().clone();
        let status_feed = StatusFeed::<NodeId> {
            last_added_block: Some(Block::doc_example().clone()),
            peers: peers_for_status,
            chainspec_info,
            version: crate::VERSION_STRING.as_str(),
        };

        let mut response_result = GetStatusResult::from(status_feed);
        response_result.set_api_version(CLIENT_API_VERSION.clone());

        result.push_without_params::<GetStatus>(
            "Retrieves the current status of the node.",
            "https://docs.rs/casper-node/latest/casper_node/rpcs/info/struct.GetStatusResult.html",
            response_result,
        );

        // Setup GetBalance.
        let request_params = GetBalanceParams {
            state_root_hash: Digest::from([4u8; Digest::LENGTH]),
            purse_uref: String::from(
            "uref-09480c3248ef76b603d386f3f4f8a5f87f597d4eaffd475433f861af187ab5db-007",
            ),
        };

        let response_result = GetBalanceResult {
            api_version: CLIENT_API_VERSION.clone(),
            balance_value: U512::from(1234567),
            merkle_proof: MERKLE_PROOF.clone(),
        };

        result.push_with_params::<GetBalance>(
            "Retrieves a purse's balance from the network.",
            "https://docs.rs/casper-node/latest/casper_node/rpcs/state/struct.GetBalanceParams.html",
            request_params,
            "https://docs.rs/casper-node/latest/casper_node/rpcs/state/struct.GetBalanceResult.html",
            response_result,
        );

        // Setup GetAuctionInfo.
        let auction_info = AuctionState::doc_example().clone();
        let response_result = GetAuctionInfoResult {
            api_version: CLIENT_API_VERSION.clone(),
            auction_state: auction_info,
        };

        result.push_without_params::<GetAuctionInfo> (
            "Retrieves the bids and validators as of the most recently added Block.",
            "https://docs.rs/casper-node/latest/casper_node/rpcs/state/struct.GetAuctionInfoResult.html",
            response_result,
        );

        // Setup GetItem Result.
        let request_params = GetItemParams {
            state_root_hash: Digest::from([5u8; Digest::LENGTH]),
            key: String::from("014c3a26eacaf15280041314f0a13d62d177c23a0d8220d81986f306b86fbce5b6"),
            path: vec![],
        };

        let cl_value = CLValue::from_t(1u64).unwrap();

        let response_result = GetItemResult {
            api_version: CLIENT_API_VERSION.clone(),
            stored_value: StoredValue::CLValue(cl_value),
            merkle_proof: MERKLE_PROOF.clone(),
        };

        result.push_with_params::<GetItem>(
            "Retrieves a stored value from the network",
            "https://docs.rs/casper-node/latest/casper_node/rpcs/state/struct.GetItemsParams.html",
            request_params,
            "https://docs.rs/casper-node/latest/casper_node/rpcs/state/struct.GetItemsResult.html",
            response_result,
        );

        result
    };
}
/// A trait used to generate a static hardcoded example of `Self`.
pub trait DocExample {
    /// Generates a hardcoded example of `Self`.
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
    example: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResponseResult {
    docs_url: Option<String>,
    example: Value,
}

#[derive(Debug, Serialize, Deserialize)]
/// The struct containing the documentation for the RPCs.
pub struct RpcDocs {
    method: String,
    summary: String,
    request_params: RequestParams,
    response_result: ResponseResult,
}

/// Result for "docs_get_rpcs" RPC response.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRpcsResult {
    /// The RPC API version.
    api_version: Version,
    /// The list of supported RPCs.
    rpcs: Vec<RpcDocs>,
}

impl GetRpcsResult {
    fn push_with_params<T: RpcWithParams>(
        &mut self,
        summary: &str,
        request_params_docs_url: &str,
        request_params_example: T::RequestParams,
        response_result_docs_url: &str,
        response_result_example: T::ResponseResult,
    ) {
        let request_params = RequestParams {
            requirement: String::from("params must be present"),
            docs_url: Some(request_params_docs_url.to_string()),
            example: Some(json!(request_params_example)),
        };

        let response_result = ResponseResult {
            docs_url: Some(response_result_docs_url.to_string()),
            example: json!(response_result_example),
        };

        let docs = RpcDocs {
            method: T::METHOD.to_string(),
            summary: summary.to_string(),
            request_params,
            response_result,
        };

        self.rpcs.push(docs);
    }

    fn push_with_optional_params<T: RpcWithOptionalParams>(
        &mut self,
        summary: &str,
        request_params_docs_url: &str,
        request_params_example: T::OptionalRequestParams,
        response_result_docs_url: &str,
        response_result_example: T::ResponseResult,
    ) {
        let request_params = RequestParams {
            requirement: "params may or may not be present".to_string(),
            docs_url: Some(request_params_docs_url.to_string()),
            example: Some(json!(request_params_example)),
        };

        let response_result = ResponseResult {
            docs_url: Some(response_result_docs_url.to_string()),
            example: json!(response_result_example),
        };

        let docs = RpcDocs {
            method: T::METHOD.to_string(),
            summary: summary.to_string(),
            request_params,
            response_result,
        };

        self.rpcs.push(docs);
    }

    fn push_without_params<T: RpcWithoutParams>(
        &mut self,
        summary: &str,
        response_result_docs_url: &str,
        response_result_example: T::ResponseResult,
    ) {
        let request_params = RequestParams {
            requirement: "params should not be present".to_string(),
            docs_url: None,
            example: None,
        };

        let response_result = ResponseResult {
            docs_url: Some(response_result_docs_url.to_string()),
            example: json!(response_result_example),
        };

        let docs = RpcDocs {
            method: T::METHOD.to_string(),
            summary: summary.to_string(),
            request_params,
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
