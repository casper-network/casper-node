//! RPCs related to accounts.

use std::str;

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tracing::info;
use warp_json_rpc::Builder;

use super::{ApiRequest, Error, ErrorCode, ReactorEventT, RpcWithParams, RpcWithParamsExt};
use crate::{
    components::api_server::CLIENT_API_VERSION, effect::EffectBuilder, reactor::QueueKind,
    types::Deploy,
};

/// Params for "account_put_deploy" RPC request.
#[derive(Serialize, Deserialize, Debug)]
pub struct PutDeployParams {
    /// JSON-encoded deploy.
    pub deploy: Map<String, Value>,
}

/// Result for "account_put_deploy" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct PutDeployResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The deploy hash.
    pub deploy_hash: String,
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
            // Try to parse a deploy from the params.
            let json_deploy = Value::Object(params.deploy);
            let deploy = match Deploy::from_json(json_deploy).map_err(|error| error.to_string()) {
                Ok(deploy) => deploy,
                Err(error_msg) => {
                    info!("failed to put deploy: {}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::ParseDeploy as i64,
                        error_msg,
                    ))?);
                }
            };

            let deploy_hash = *deploy.id();

            // Submit the new deploy to be announced.
            effect_builder
                .make_request(
                    |responder| ApiRequest::SubmitDeploy {
                        deploy: Box::new(deploy),
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            // Return the result.
            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                deploy_hash: hex::encode(deploy_hash.inner()),
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}
