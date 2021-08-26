//! RPCs returning ancillary information.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::str;

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::info;
use warp_json_rpc::Builder;

use casper_types::{EraId, ExecutionResult, ProtocolVersion, PublicKey};

use super::{
    docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
    Error, ErrorCode, ReactorEventT, RpcRequest, RpcWithParams, RpcWithParamsExt, RpcWithoutParams,
    RpcWithoutParamsExt,
};
use crate::{
    components::consensus::ValidatorChange,
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{Block, BlockHash, Deploy, DeployHash, GetStatusResult, Item, PeersMap},
};

static GET_DEPLOY_PARAMS: Lazy<GetDeployParams> = Lazy::new(|| GetDeployParams {
    deploy_hash: *Deploy::doc_example().id(),
});
static GET_DEPLOY_RESULT: Lazy<GetDeployResult> = Lazy::new(|| GetDeployResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    deploy: Deploy::doc_example().clone(),
    execution_results: vec![JsonExecutionResult {
        block_hash: Block::doc_example().id(),
        result: ExecutionResult::example().clone(),
    }],
});
static GET_PEERS_RESULT: Lazy<GetPeersResult> = Lazy::new(|| GetPeersResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    peers: GetStatusResult::doc_example().peers.clone(),
});
static GET_VALIDATOR_INFO_RESULT: Lazy<GetValidatorInfoResult> =
    Lazy::new(|| GetValidatorInfoResult {
        api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
        validator_info: vec![],
    });

/// Params for "info_get_deploy" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDeployParams {
    /// The deploy hash.
    pub deploy_hash: DeployHash,
}

impl DocExample for GetDeployParams {
    fn doc_example() -> &'static Self {
        &*GET_DEPLOY_PARAMS
    }
}

/// The execution result of a single deploy.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JsonExecutionResult {
    /// The block hash.
    pub block_hash: BlockHash,
    /// Execution result.
    pub result: ExecutionResult,
}

/// Result for "info_get_deploy" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDeployResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The deploy.
    pub deploy: Deploy,
    /// The map of block hash to execution result.
    pub execution_results: Vec<JsonExecutionResult>,
}

impl DocExample for GetDeployResult {
    fn doc_example() -> &'static Self {
        &*GET_DEPLOY_RESULT
    }
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
        api_version: ProtocolVersion,
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
                .map(|(block_hash, result)| JsonExecutionResult { block_hash, result })
                .collect();

            let result = Self::ResponseResult {
                api_version,
                deploy,
                execution_results,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}

/// Result for "info_get_peers" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetPeersResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The node ID and network address of each connected peer.
    pub peers: PeersMap,
}

impl DocExample for GetPeersResult {
    fn doc_example() -> &'static Self {
        &*GET_PEERS_RESULT
    }
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
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            let peers = effect_builder
                .make_request(
                    |responder| RpcRequest::GetPeers { responder },
                    QueueKind::Api,
                )
                .await;

            let result = Self::ResponseResult {
                api_version,
                peers: PeersMap::from(peers),
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
        api_version: ProtocolVersion,
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
            let body = Self::ResponseResult::new(status_feed, api_version);
            Ok(response_builder.success(body)?)
        }
        .boxed()
    }
}

/// A change to a validator's status between two eras.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JsonEraChanges {
    /// The era in which the change occurred.
    era_id: EraId,
    /// The change in status.
    validator_change: ValidatorChange,
}

/// The information for a validator across eras.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JsonValidatorInfo {
    /// The public key of the given validator.
    public_key: PublicKey,
    /// The set of changes to the validator's status.
    era_changes: Vec<JsonEraChanges>,
}

/// Result for the "info_get_validator_info" RPC.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetValidatorInfoResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The validator information.
    pub validator_info: Vec<JsonValidatorInfo>,
}

impl DocExample for GetValidatorInfoResult {
    fn doc_example() -> &'static Self {
        &*GET_VALIDATOR_INFO_RESULT
    }
}

/// "info_get_validator_info" RPC.
pub struct GetValidatorInfo {}

impl RpcWithoutParams for GetValidatorInfo {
    const METHOD: &'static str = "info_get_validator_info";
    type ResponseResult = GetValidatorInfoResult;
}

impl RpcWithoutParamsExt for GetValidatorInfo {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            // Get the validator info.
            let validator_info = effect_builder
                .get_consensus_validator_info()
                .await
                .into_iter()
                .map(|(public_key, era_changes)| {
                    let era_changes = era_changes
                        .into_iter()
                        .map(|(era_id, validator_change)| JsonEraChanges {
                            era_id,
                            validator_change,
                        })
                        .collect();
                    JsonValidatorInfo {
                        public_key,
                        era_changes,
                    }
                })
                .collect();
            let result = Self::ResponseResult {
                api_version,
                validator_info,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}
