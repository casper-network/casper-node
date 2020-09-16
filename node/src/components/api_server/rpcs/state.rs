use std::{convert::TryFrom, str};

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::info;
use warp_json_rpc::Builder;

use casper_execution_engine::core::engine_state::QueryResult;
use casper_types::Key;

use super::{ApiRequest, Error, ErrorCode, ReactorEventT, RpcWithParams};
use crate::{
    components::api_server::CLIENT_API_VERSION, crypto::hash::Digest, effect::EffectBuilder,
    reactor::QueueKind, types::json_compatibility::StoredValue,
};

#[derive(Deserialize)]
pub(in crate::components::api_server) struct GetItemParams {
    /// The global state hash.
    global_state_hash: String,
    /// Hex-encoded `casper_types::Key`.
    key: String,
    /// The path components starting from the key as base.
    path: Vec<String>,
}

pub(in crate::components::api_server) struct GetItem {}

impl RpcWithParams for GetItem {
    const METHOD: &'static str = "state_get_item";

    type RequestParams = GetItemParams;

    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        /// The JSON-RPC response's "result".
        #[derive(Serialize)]
        struct ResponseResult {
            api_version: Version,
            stored_value: StoredValue,
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

            // Try to parse a `casper_types::Key` from the params.
            let base_key = match Key::from_formatted_str(&params.key)
                .map_err(|error| format!("failed to parse key: {:?}", error))
            {
                Ok(key) => key,
                Err(error_msg) => {
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::ParseQueryKey as i64,
                        error_msg,
                    ))?);
                }
            };

            // Run the query.
            let query_result = effect_builder
                .make_request(
                    |responder| ApiRequest::QueryGlobalState {
                        global_state_hash,
                        base_key,
                        path: params.path,
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            // Extract the EE `StoredValue` from the result.
            let ee_stored_value = match query_result {
                Ok(QueryResult::Success(stored_value)) => stored_value,
                Ok(query_result) => {
                    let error_msg = format!("state query failed: {:?}", query_result);
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::QueryFailed as i64,
                        error_msg,
                    ))?);
                }
                Err(error) => {
                    let error_msg = format!("state query failed to execute: {}", error);
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::QueryFailedToExecute as i64,
                        error_msg,
                    ))?);
                }
            };

            // Return the result.
            match StoredValue::try_from(&ee_stored_value) {
                Ok(stored_value) => {
                    let result = ResponseResult {
                        api_version: CLIENT_API_VERSION.clone(),
                        stored_value,
                    };
                    Ok(response_builder.success(result)?)
                }
                Err(error) => {
                    info!("failed to encode stored value: {}", error);
                    return Ok(response_builder.error(warp_json_rpc::Error::INTERNAL_ERROR)?);
                }
            }
        }
        .boxed()
    }
}
