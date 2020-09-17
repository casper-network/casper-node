use std::str;

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::info;
use warp_json_rpc::Builder;

use casper_execution_engine::core::engine_state::BalanceResult;
use casper_types::{Key, U512};

use super::{ApiRequest, Error, ErrorCode, ReactorEventT, RpcWithParams};
use crate::{
    components::api_server::CLIENT_API_VERSION, crypto::hash::Digest, effect::EffectBuilder,
    reactor::QueueKind,
};

#[derive(Deserialize)]
pub(in crate::components::api_server) struct GetBalanceParams {
    /// The global state hash.
    global_state_hash: String,
    /// Hex-encoded `casper_types::Key`.
    purse_key: String,
}

pub(in crate::components::api_server) struct GetBalance {}

impl RpcWithParams for GetBalance {
    const METHOD: &'static str = "get_balance";

    type RequestParams = GetBalanceParams;

    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        /// The JSON-RPC response's "result".
        #[derive(Serialize)]
        struct ResponseResult {
            api_version: Version,
            balance_value: U512,
        }

        async move {
            // Try to parse the global state hash from the params.
            let global_state_hash = match Digest::from_hex(&params.global_state_hash)
                .map_err(|error| format!("failed to parse global state hash: {}", error))
            {
                Ok(hash) => hash,
                Err(error_msg) => {
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::ParseBlockHash as i64,
                        error_msg,
                    ))?);
                }
            };

            // Try to parse a `casper_types::Key` representing the purse from the params.
            let purse_key = match Key::from_formatted_str(&params.purse_key)
                .map_err(|error| format!("failed to parse purse_key: {:?}", error))
            {
                Ok(key) => key,
                Err(error_msg) => {
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::ParseGetBalanceKey as i64,
                        error_msg,
                    ))?);
                }
            };

            // Get the balance.
            let balance_result = effect_builder
                .make_request(
                    |responder| ApiRequest::GetBalance {
                        global_state_hash,
                        purse_key,
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            let balance_value = match balance_result {
                Ok(BalanceResult::Success(value)) => value,
                Ok(balance_result) => {
                    let error_msg = format!("get-balance failed: {:?}", balance_result);
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::GetBalanceFailed as i64,
                        error_msg,
                    ))?);
                }
                Err(error) => {
                    let error_msg = format!("get-balance failed to execute: {}", error);
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::GetBalanceFailedToExecute as i64,
                        error_msg,
                    ))?);
                }
            };

            // Return the result.
            let result = ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                balance_value,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}
