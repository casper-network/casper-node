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
use serde_json::Value;
use tracing::info;
use warp_json_rpc::Builder;

use super::{
    ApiRequest, Error, ErrorCode, ReactorEventT, RpcWithParams, RpcWithParamsExt, RpcWithoutParams,
    RpcWithoutParamsExt,
};
use crate::{
    components::{api_server::CLIENT_API_VERSION, small_network::NodeId},
    crypto::hash::Digest,
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{json_compatibility::ExecutionResult, DeployHash, BlockHash},
};

/// Params for "info_get_deploy" RPC request.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetDeployParams {
    /// Hex-encoded deploy hash.
    pub deploy_hash: String,
}
/// Params for "info_get_block" RPC request. 
#[derive(Serialize, Deserialize, Debug)]
pub struct GetBlockParams {
    /// Hex-encoded block hash.
    pub block_hash: String,
}


/// The execution result of a single deploy.
#[derive(Serialize, Deserialize, Debug)]
pub struct JsonExecutionResult {
    /// Hex-encoded block hash.
    pub block_hash: String,
    /// Execution result.
    pub result: ExecutionResult,
}

/// Result for "info_get_deploy" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetDeployResult {
    /// The RPC API version.
    pub api_version: Version,
    /// JSON-encoded deploy.
    pub deploy: Value,
    /// The map of block hash to execution result.
    pub execution_results: Vec<JsonExecutionResult>,
}

/// Result for "info_get_block" RPC response. 
#[derive(Serialize, Deserialize, Debug)]
pub struct GetBlockResult {
    /// The RPC API version.
    pub api_version: Version,
    /// JSON-encoded deploy.
    pub block: Value,
}

/// "info_get_deploy" RPC.
pub struct GetDeploy {}

/// "info_get_block" RPC. 
pub struct GetBlock {}

impl RpcWithParams for GetDeploy {
    const METHOD: &'static str = "info_get_deploy";
    type RequestParams = GetDeployParams;
    type ResponseResult = GetDeployResult;
}

impl RpcWithParams for GetBlock {
    const METHOD: &'static str = "info_get_block";
    type RequestParams = GetBlockParams;
    type ResponseResult = GetBlockResult;
}

impl RpcWithParamsExt for GetBlock {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // Try to parse the block hash from the params
            let block_hash = 
                match Digest::from_hex(&params.block_hash).map_err(|error| error.to_string()) {
                    Ok(digest) => BlockHash::new(digest),
                    Err(error_msg) => {
                        info!("failed to get deploy: {}", error_msg);
                        return Ok(response_builder.error(warp_json_rpc::Error::custom(
                            ErrorCode::ParseDeployHash as i64,
                            error_msg,
                        ))?);
                    }
                };
            // Try to get the deploy and metadata from storage.
            let maybe_block = effect_builder
                .make_request(
                    |responder| ApiRequest::GetBlock {
                        maybe_hash: Some(block_hash),
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;
            
            let block = match maybe_block {
                Some(block) => block,
                None => {
                    info!("failed to get {} and metadata from storage", block_hash);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::NoSuchBlock as i64,
                        "block not known",
                    ))?);
                }
            };

            let block_as_json = block.to_json();
            /*
            let execution_results = metadata
                .execution_results
                .into_iter()
                .map(|(block_hash, result)| JsonExecutionResult {
                    block_hash: hex::encode(block_hash.as_ref()),
                    result,
                })
                .collect();
            */
            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                block: block_as_json,
                //execution_results,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

impl RpcWithParamsExt for GetDeploy {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // Try to parse a deploy hash from the params.
            let deploy_hash =
                match Digest::from_hex(&params.deploy_hash).map_err(|error| error.to_string()) {
                    Ok(digest) => DeployHash::new(digest),
                    Err(error_msg) => {
                        info!("failed to get deploy: {}", error_msg);
                        return Ok(response_builder.error(warp_json_rpc::Error::custom(
                            ErrorCode::ParseDeployHash as i64,
                            error_msg,
                        ))?);
                    }
                };

            // Try to get the deploy and metadata from storage.
            let maybe_deploy_and_metadata = effect_builder
                .make_request(
                    |responder| ApiRequest::GetDeploy {
                        hash: deploy_hash,
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            let (deploy, metadata) = match maybe_deploy_and_metadata {
                Some((deploy, metadata)) => (deploy, metadata),
                None => {
                    info!("failed to get {} and metadata from storage", deploy_hash);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::NoSuchDeploy as i64,
                        "deploy not known",
                    ))?);
                }
            };

            // Return the result.
            let deploy_as_json = deploy.to_json();
            let execution_results = metadata
                .execution_results
                .into_iter()
                .map(|(block_hash, result)| JsonExecutionResult {
                    block_hash: hex::encode(block_hash.as_ref()),
                    result,
                })
                .collect();

            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                deploy: deploy_as_json,
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

/// Result for "info_get_status" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetStatusResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The node ID and network address of each connected peer.
    pub peers: BTreeMap<String, SocketAddr>,
    /// The last block from the linear chain, JSON-encoded.
    pub last_finalized_block: Option<Value>,
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
            let peers = peers_hashmap_to_btreemap(status_feed.peers);
            let last_finalized_block = status_feed
                .last_finalized_block
                .map(|block| block.to_json());

            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                peers,
                last_finalized_block,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

/// Result for "info_get_metrics" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetMetricsResult {
    /// The RPC API version.
    pub api_version: Version,
    /// JSON-encoded metrics.
    pub metrics: String,
}

/// "info_get_metrics" RPC.
pub struct GetMetrics {}

impl RpcWithoutParams for GetMetrics {
    const METHOD: &'static str = "info_get_metrics";
    type ResponseResult = GetMetricsResult;
}

impl RpcWithoutParamsExt for GetMetrics {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            let maybe_metrics = effect_builder
                .make_request(
                    |responder| ApiRequest::GetMetrics { responder },
                    QueueKind::Api,
                )
                .await;

            match maybe_metrics {
                Some(metrics) => {
                    let result = Self::ResponseResult {
                        api_version: CLIENT_API_VERSION.clone(),
                        metrics,
                    };
                    Ok(response_builder.success(result)?)
                }
                None => {
                    info!("metrics not available");
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::MetricsNotAvailable as i64,
                        "metrics not available",
                    ))?);
                }
            }
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
