use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    str,
};

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::Serialize;
use tracing::info;
use warp_json_rpc::Builder;

use super::{ApiRequest, Error, ErrorCode, ReactorEventT, RpcWithParams, RpcWithoutParams};
use crate::{
    components::{api_server::CLIENT_API_VERSION, small_network::NodeId},
    crypto::hash::Digest,
    effect::EffectBuilder,
    reactor::QueueKind,
    types::DeployHash,
};

pub(in crate::components::api_server) struct GetDeploy {}

impl RpcWithParams for GetDeploy {
    const METHOD: &'static str = "info_get_deploy";

    type RequestParams = String; // Deploy hash.

    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        /// The JSON-RPC response's "result".
        #[derive(Serialize)]
        struct ResponseResult {
            api_version: Version,
            /// JSON-encoded deploy.
            deploy: String,
        }

        async move {
            // Try to parse a deploy hash from the params.
            let deploy_hash = match Digest::from_hex(&params).map_err(|error| error.to_string()) {
                Ok(digest) => DeployHash::new(digest),
                Err(error_msg) => {
                    info!("failed to get deploy: {}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::ParseDeployHash as i64,
                        error_msg,
                    ))?);
                }
            };

            // Try to get the deploy from storage.
            let maybe_deploy = effect_builder
                .make_request(
                    |responder| ApiRequest::GetDeploy {
                        hash: deploy_hash,
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            let deploy = match maybe_deploy {
                Some(deploy) => deploy,
                None => {
                    info!("failed to get {} from storage", deploy_hash);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::NoSuchDeploy as i64,
                        "deploy not known",
                    ))?);
                }
            };

            // Return the result.
            match deploy.to_json() {
                Ok(deploy_as_json) => {
                    let result = ResponseResult {
                        api_version: CLIENT_API_VERSION.clone(),
                        deploy: deploy_as_json,
                    };
                    Ok(response_builder.success(result)?)
                }
                Err(error) => {
                    info!("failed to encode deploy to JSON: {}", error);
                    return Ok(response_builder.error(warp_json_rpc::Error::INTERNAL_ERROR)?);
                }
            }
        }
        .boxed()
    }
}

pub(in crate::components::api_server) struct GetPeers {}

impl RpcWithoutParams for GetPeers {
    const METHOD: &'static str = "info_get_peers";

    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        /// The JSON-RPC response's "result".
        #[derive(Serialize)]
        struct ResponseResult {
            api_version: Version,
            peers: BTreeMap<String, SocketAddr>,
        }

        async move {
            let peers = effect_builder
                .make_request(
                    |responder| ApiRequest::GetPeers { responder },
                    QueueKind::Api,
                )
                .await;

            let peers = peers_hashmap_to_btreemap(peers);
            let result = ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                peers,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

pub(in crate::components::api_server) struct GetStatus {}

impl RpcWithoutParams for GetStatus {
    const METHOD: &'static str = "info_get_status";

    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        /// The JSON-RPC response's "result".
        #[derive(Serialize)]
        struct ResponseResult {
            api_version: Version,
            /// The last block from the linear chain, JSON-encoded.
            last_finalized_block: Option<String>,
            /// The connected peers' Node IDs and network addresses.
            peers: BTreeMap<String, SocketAddr>,
        }

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
            let last_finalized_block = match status_feed.last_finalized_block {
                Some(block) => match block.to_json() {
                    Ok(block_as_json) => Some(block_as_json),
                    Err(error) => {
                        info!("failed to encode block to JSON: {}", error);
                        return Ok(response_builder.error(warp_json_rpc::Error::INTERNAL_ERROR)?);
                    }
                },
                None => None,
            };

            let result = ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                peers,
                last_finalized_block,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

pub(in crate::components::api_server) struct GetMetrics {}

impl RpcWithoutParams for GetMetrics {
    const METHOD: &'static str = "info_get_metrics";

    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        /// The JSON-RPC response's "result".
        #[derive(Serialize)]
        struct ResponseResult {
            api_version: Version,
            metrics: String,
        }

        async move {
            let maybe_metrics = effect_builder
                .make_request(
                    |responder| ApiRequest::GetMetrics { responder },
                    QueueKind::Api,
                )
                .await;

            match maybe_metrics {
                Some(metrics) => {
                    let result = ResponseResult {
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
