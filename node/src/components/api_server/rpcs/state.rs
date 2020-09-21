//! RPCs related to the global state.

use std::{convert::TryFrom, str};

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::info;
use warp_json_rpc::Builder;

use casper_execution_engine::core::engine_state::{BalanceResult, QueryResult};
use casper_types::{Key, URef, U512};

use super::{ApiRequest, Error, ErrorCode, ReactorEventT, RpcWithParams, RpcWithParamsExt};
use crate::{
    components::api_server::CLIENT_API_VERSION, crypto::hash::Digest, effect::EffectBuilder,
    reactor::QueueKind, types::json_compatibility::StoredValue,
};

/// Params for "state_get_item" RPC request.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetItemParams {
    /// Hex-encoded global state hash.
    pub global_state_hash: String,
    /// `casper_types::Key` as formatted string.
    pub key: String,
    /// The path components starting from the key as base.
    pub path: Vec<String>,
}

/// Result for "state_get_item" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetItemResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The stored value.
    pub stored_value: StoredValue,
}

/// "state_get_item" RPC.
pub struct GetItem {}

impl RpcWithParams for GetItem {
    const METHOD: &'static str = "state_get_item";
    type RequestParams = GetItemParams;
    type ResponseResult = GetItemResult;
}

impl RpcWithParamsExt for GetItem {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
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
                    let result = Self::ResponseResult {
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

/// Params for "state_get_balance" RPC request.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetBalanceParams {
    /// Hex-encoded global state hash.
    pub global_state_hash: String,
    /// Formatted URef.
    pub purse_uref: String,
}

/// Result for "state_get_balance" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct GetBalanceResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The balance value.
    pub balance_value: U512,
}

/// "state_get_balance" RPC.
pub struct GetBalance {}

impl RpcWithParams for GetBalance {
    const METHOD: &'static str = "state_get_balance";
    type RequestParams = GetBalanceParams;
    type ResponseResult = GetBalanceResult;
}

impl RpcWithParamsExt for GetBalance {
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
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

            // Try to parse the purse's URef from the params.
            let purse_uref = match URef::from_formatted_str(&params.purse_uref)
                .map_err(|error| format!("failed to parse purse_uref: {:?}", error))
            {
                Ok(uref) => uref,
                Err(error_msg) => {
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::ParseGetBalanceURef as i64,
                        error_msg,
                    ))?);
                }
            };

            // Get the balance.
            let balance_result = effect_builder
                .make_request(
                    |responder| ApiRequest::GetBalance {
                        global_state_hash,
                        purse_uref,
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
            let result = Self::ResponseResult {
                api_version: CLIENT_API_VERSION.clone(),
                balance_value,
            };
            Ok(response_builder.success(result)?)
        }
        .boxed()
    }
}
