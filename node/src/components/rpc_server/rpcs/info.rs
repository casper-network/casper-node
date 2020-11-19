//! RPCs returning ancillary information.

use std::{net::SocketAddr, str};

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::info;
use warp_json_rpc::Builder;

use super::{
    Error, ErrorCode, ReactorEventT, RpcRequest, RpcWithParams, RpcWithParamsExt, RpcWithoutParams,
    RpcWithoutParamsExt,
};
use crate::{
    components::CLIENT_API_VERSION,
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{
        json_compatibility::ExecutionResult as RPCExecutionResult, BlockHash, Deploy, DeployHash,
        GetStatusResult, NodeId,
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
    pub result: RPCExecutionResult,
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
                    |responder| RpcRequest::GetDeploy {
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
                .map(|(block_hash, result)| JsonExecutionResult {
                    block_hash,
                    result: result.into(),
                })
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
/// Change Peers from HashMap to OpenRPC compatible format
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonPeers {
    form: String,
    id: String,
    addr: SocketAddr,
}

impl JsonPeers {
    /// Create a single OpenRPC compatible Peer representation.
    pub fn new(node: NodeId, addr: SocketAddr) -> Self {
        match node {
            NodeId::Tls(fingerprint) => JsonPeers {
                form: "Tls".to_string(),
                id: fingerprint.to_string(),
                addr,
            },
            NodeId::P2p(peer_id) => JsonPeers {
                form: "P2p".to_string(),
                id: peer_id.to_string(),
                addr,
            },
        }
    }
}

/// Result for "info_get_peers" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetPeersResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The node ID and network address of each connected peer.
    pub peers: Vec<JsonPeers>,
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
                    |responder| RpcRequest::GetPeers { responder },
                    QueueKind::Api,
                )
                .await;

            let peers = peers
                .iter()
                .map(|(node, addr)| JsonPeers::new(node.clone(), *addr))
                .collect();
            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                peers,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
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
                    |responder| RpcRequest::GetStatus { responder },
                    QueueKind::Api,
                )
                .await;

            // Convert to `ResponseResult` and send.
            let mut body = Self::ResponseResult::from(status_feed);
            body.set_api_version(CLIENT_API_VERSION.clone());
            Ok(response_builder.success(body)?)
        }
        .boxed()
    }
}
