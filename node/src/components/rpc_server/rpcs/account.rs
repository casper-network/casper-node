//! RPCs related to accounts.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::str;

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use lazy_static::lazy_static;
use schemars::JsonSchema;
use semver::Version;
use serde::{Deserialize, Serialize};
use warp_json_rpc::Builder;

use super::{docs::DocExample, Error, ReactorEventT, RpcRequest, RpcWithParams, RpcWithParamsExt};
use crate::{
    components::CLIENT_API_VERSION,
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{Deploy, DeployHash},
};

lazy_static! {
    static ref PUT_DEPLOY_PARAMS: PutDeployParams = PutDeployParams {
        deploy: Deploy::doc_example().clone(),
    };
    static ref PUT_DEPLOY_RESULT: PutDeployResult = PutDeployResult {
        api_version: CLIENT_API_VERSION.clone(),
        deploy_hash: *Deploy::doc_example().id(),
    };
}

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
    pub api_version: Version,
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
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        async move {
            let deploy_hash = *params.deploy.id();

            // Submit the new deploy to be announced.
            effect_builder
                .make_request(
                    |responder| RpcRequest::SubmitDeploy {
                        deploy: Box::new(params.deploy),
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            // Return the result.
            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                deploy_hash,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}
