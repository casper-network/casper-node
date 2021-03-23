//! RPCs related to accounts.

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

use super::{
    docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
    Error, ReactorEventT, RpcRequest, RpcWithParams, RpcWithParamsExt,
};
use crate::{
    components::rpc_server::rpcs::ErrorCode,
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{Deploy, DeployHash},
};
use casper_types::ProtocolVersion;

static PUT_DEPLOY_PARAMS: Lazy<PutDeployParams> = Lazy::new(|| PutDeployParams {
    deploy: Deploy::doc_example().clone(),
});
static PUT_DEPLOY_RESULT: Lazy<PutDeployResult> = Lazy::new(|| PutDeployResult {
    api_version: *DOCS_EXAMPLE_PROTOCOL_VERSION,
    deploy_hash: *Deploy::doc_example().id(),
});

/// Params for "account_put_deploy" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PutDeployParams {
    /// The `Deploy`.
    pub deploy: Deploy,
}

impl DocExample for PutDeployParams {
    fn doc_example() -> &'static Self {
        &*PUT_DEPLOY_PARAMS
    }
}

/// Result for "account_put_deploy" RPC response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PutDeployResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The deploy hash.
    pub deploy_hash: DeployHash,
}

impl DocExample for PutDeployResult {
    fn doc_example() -> &'static Self {
        &*PUT_DEPLOY_RESULT
    }
}

/// "account_put_deploy" RPC
pub struct PutDeploy {}

impl RpcWithParams for PutDeploy {
    const METHOD: &'static str = "account_put_deploy";
    type RequestParams = PutDeployParams;
    type ResponseResult = PutDeployResult;
}

impl RpcWithParamsExt for PutDeploy {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            let deploy_hash = *params.deploy.id();

            // Submit the new deploy to be announced.
            let put_deploy_result = effect_builder
                .make_request(
                    |responder| RpcRequest::SubmitDeploy {
                        deploy: Box::new(params.deploy),
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            match put_deploy_result {
                Ok(_) => {
                    info!(%deploy_hash,
                    "deploy was stored"
                    );
                    let result = Self::ResponseResult {
                        api_version,
                        deploy_hash,
                    };
                    Ok(response_builder.success(result)?)
                }
                Err(error) => {
                    info!(
                        %deploy_hash,
                        %error,
                        "the deploy submitted by the client was invalid",
                    );
                    Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::InvalidDeploy as i64,
                        error.to_string(),
                    ))?)
                }
            }
        }
        .boxed()
    }
}
