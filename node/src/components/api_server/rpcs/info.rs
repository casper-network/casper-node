//! RPCs returning ancillary information.

use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    str,
};

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::info;
use warp_json_rpc::Builder;

use super::{
    ApiRequest, Error, ErrorCode, ReactorEventT, RpcWithParams, RpcWithParamsExt, RpcWithoutParams,
    RpcWithoutParamsExt,
};
use crate::{
    components::{api_server::CLIENT_API_VERSION, consensus::EraId, small_network::NodeId},
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{
        json_compatibility::ExecutionResult, Block, BlockHash, Deploy, DeployHash, StatusFeed,
        Timestamp,
    },
};

/// Params for "info_get_deploy" RPC request.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetDeployParams {
    /// The deploy hash.
    pub deploy_hash: DeployHash,
}

/// The execution result of a single deploy.
#[derive(Serialize, Deserialize, Debug)]
pub struct JsonExecutionResult {
    /// The block hash.
    pub block_hash: BlockHash,
    /// Execution result.
    pub result: ExecutionResult,
}

/// Result for "info_get_deploy" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetDeployResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The deploy.
    pub deploy: Deploy,
    /// The map of block hash to execution result.
    pub execution_results: Vec<JsonExecutionResult>,
}

/// "info_get_deploy" RPC.
pub struct GetDeploy {}

impl RpcWithParams for GetDeploy {
    const METHOD: &'static str = "info_get_deploy";
    type RequestParams = GetDeployParams;
    type ResponseResult = GetDeployResult;
}

impl RpcWithParamsExt for GetDeploy {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // Try to get the deploy and metadata from storage.
            let maybe_deploy_and_metadata = effect_builder
                .make_request(
                    |responder| ApiRequest::GetDeploy {
                        hash: params.deploy_hash,
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            let (deploy, metadata) = match maybe_deploy_and_metadata {
                Some((deploy, metadata)) => (deploy, metadata),
                None => {
                    info!(
                        "failed to get {} and metadata from storage",
                        params.deploy_hash
                    );
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::NoSuchDeploy as i64,
                        "deploy not known",
                    ))?);
                }
            };

            // Return the result.
            let execution_results = metadata
                .execution_results
                .into_iter()
                .map(|(block_hash, result)| JsonExecutionResult { block_hash, result })
                .collect();

            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                deploy,
                execution_results,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

/// Result for "info_get_peers" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetPeersResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The node ID and network address of each connected peer.
    pub peers: BTreeMap<String, SocketAddr>,
}

/// "info_get_peers" RPC.
pub struct GetPeers {}

impl RpcWithoutParams for GetPeers {
    const METHOD: &'static str = "info_get_peers";
    type ResponseResult = GetPeersResult;
}

impl RpcWithoutParamsExt for GetPeers {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            let peers = effect_builder
                .make_request(
                    |responder| ApiRequest::GetPeers { responder },
                    QueueKind::Api,
                )
                .await;

            let peers = peers_hashmap_to_btreemap(peers);
            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                peers,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

/// Minimal info of a `Block`.
#[derive(Serialize, Deserialize, Debug)]
pub struct MinimalBlockInfo {
    hash: BlockHash,
    timestamp: Timestamp,
    era_id: EraId,
    height: u64,
}

impl From<Block> for MinimalBlockInfo {
    fn from(block: Block) -> Self {
        MinimalBlockInfo {
            hash: *block.hash(),
            timestamp: block.header().timestamp(),
            era_id: block.header().era_id(),
            height: block.header().height(),
        }
    }
}

/// Result for "info_get_status" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetStatusResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The chainspec name.
    pub chainspec_name: String,
    /// The genesis root hash.
    pub genesis_root_hash: String,
    /// The node ID and network address of each connected peer.
    pub peers: BTreeMap<String, SocketAddr>,
    /// The minimal info of the last block from the linear chain.
    pub last_finalized_block_info: Option<MinimalBlockInfo>,
}

impl From<StatusFeed<NodeId>> for GetStatusResult {
    fn from(status_feed: StatusFeed<NodeId>) -> Self {
        let chainspec_name = status_feed.chainspec_info.name();
        let genesis_root_hash = status_feed
            .chainspec_info
            .root_hash()
            .unwrap_or_default()
            .to_string();
        GetStatusResult {
            api_version: CLIENT_API_VERSION.clone(),
            chainspec_name,
            genesis_root_hash,
            peers: peers_hashmap_to_btreemap(status_feed.peers),
            last_finalized_block_info: status_feed.last_finalized_block.map(Into::into),
        }
    }
}

/// "info_get_status" RPC.
pub struct GetStatus {}

impl RpcWithoutParams for GetStatus {
    const METHOD: &'static str = "info_get_status";
    type ResponseResult = GetStatusResult;
}

impl RpcWithoutParamsExt for GetStatus {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // Get the status.
            let status_feed = effect_builder
                .make_request(
                    |responder| ApiRequest::GetStatus { responder },
                    QueueKind::Api,
                )
                .await;

            // Convert to `ResponseResult` and send.
            let result = Self::ResponseResult::from(status_feed);
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

fn peers_hashmap_to_btreemap(peers: HashMap<NodeId, SocketAddr>) -> BTreeMap<String, SocketAddr> {
    peers
        .into_iter()
        .map(|(node_id, address)| (format!("{}", node_id), address))
        .collect()
}
