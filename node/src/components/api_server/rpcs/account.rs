//! RPCs related to accounts.

use std::str;

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::{Deserialize, Serialize};
use warp_json_rpc::Builder;

use super::{ApiRequest, Error, ReactorEventT, RpcWithParams, RpcWithParamsExt};
use crate::{
    components::api_server::CLIENT_API_VERSION,
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{Deploy, DeployHash},
};

/// Params for "account_put_deploy" RPC request.
#[derive(Serialize, Deserialize, Debug)]
pub struct PutDeployParams {
    /// The `Deploy`.
    pub deploy: Deploy,
}

/// Result for "account_put_deploy" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct PutDeployResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The deploy hash.
    pub deploy_hash: DeployHash,
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
                    |responder| ApiRequest::SubmitDeploy {
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
