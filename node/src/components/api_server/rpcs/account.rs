use std::str;

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::Serialize;
use serde_json::{Map, Value};
use tracing::info;
use warp_json_rpc::Builder;

use super::{ApiRequest, Error, ErrorCode, ReactorEventT, RpcWithParams};
use crate::{
    components::api_server::CLIENT_API_VERSION, effect::EffectBuilder, reactor::QueueKind,
    types::Deploy,
};

pub(in crate::components::api_server) struct PutDeploy {}

impl RpcWithParams for PutDeploy {
    const METHOD: &'static str = "account_put_deploy";

    type RequestParams = Map<String, Value>; // JSON-encoded deploy.

    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        /// The JSON-RPC response's "result".
        #[derive(Serialize)]
        struct ResponseResult {
            api_version: Version,
            deploy_hash: String,
        }

        async move {
            // Try to parse a deploy from the params.
            let json_deploy = Value::Object(params);
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
            let result = ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                deploy_hash: hex::encode(deploy_hash.inner()),
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}
