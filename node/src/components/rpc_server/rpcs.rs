//! The set of JSON-RPCs which the API server handles.
//!
//! See <https://github.com/CasperLabs/ceps/blob/master/text/0009-client-api.md#rpcs> for info.

pub mod account;
pub mod chain;
mod common;
pub mod docs;
pub mod info;
pub mod state;

use std::str;

use futures::{future::BoxFuture, TryFutureExt};
use http::Response;
use hyper::Body;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use warp::{
    filters::BoxedFilter,
    reject::{self, Reject},
    Filter,
};
use warp_json_rpc::{filters, Builder};

use casper_types::ProtocolVersion;

use super::{ReactorEventT, RpcRequest};
use crate::effect::EffectBuilder;
pub use common::ErrorData;
use docs::DocExample;

/// The URL path.
pub const RPC_API_PATH: &str = "rpc";

/// Error code returned if the JSON-RPC response indicates failure.
///
/// See <https://www.jsonrpc.org/specification#error_object> for details.
#[repr(i64)]
enum ErrorCode {
    NoSuchDeploy = -32000,
    NoSuchBlock = -32001,
    ParseQueryKey = -32002,
    QueryFailed = -32003,
    QueryFailedToExecute = -32004,
    ParseGetBalanceURef = -32005,
    GetBalanceFailed = -32006,
    GetBalanceFailedToExecute = -32007,
    InvalidDeploy = -32008,
    NoSuchAccount = -32009,
    FailedToGetDictionaryURef = -32010,
    FailedToGetTrie = -32011,
    NoSuchStateRoot = -32012,
    // Same error code as warp_json INTERNAL_ERROR.
    InternalError = -32063,
}

#[derive(Debug)]
pub(super) struct Error(String);

impl Reject for Error {}

impl From<anyhow::Error> for Error {
    fn from(error: anyhow::Error) -> Self {
        Error(error.to_string())
    }
}

/// A JSON-RPC requiring the "params" field to be present.
pub trait RpcWithParams {
    /// The JSON-RPC "method" name.
    const METHOD: &'static str;

    /// The JSON-RPC request's "params" type.
    type RequestParams: Serialize
        + for<'de> Deserialize<'de>
        + JsonSchema
        + DocExample
        + Send
        + 'static;

    /// The JSON-RPC response's "result" type.
    type ResponseResult: Serialize
        + for<'de> Deserialize<'de>
        + JsonSchema
        + DocExample
        + Send
        + 'static;
}

/// A trait for creating a JSON-RPC filter where the request is required to have "params".
pub(super) trait RpcWithParamsExt: RpcWithParams {
    /// Creates the warp filter for this particular RPC.
    fn create_filter<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
    ) -> BoxedFilter<(Response<Body>,)> {
        let with_valid_params = warp::path(RPC_API_PATH)
            .and(filters::json_rpc())
            .and(filters::method(Self::METHOD))
            .and(filters::params::<Self::RequestParams>())
            .and_then(
                move |response_builder: Builder, params: Self::RequestParams| {
                    Self::handle_request(effect_builder, response_builder, params, api_version)
                        .map_err(reject::custom)
                },
            );
        let with_invalid_params = warp::path(RPC_API_PATH)
            .and(filters::json_rpc())
            .and(filters::method(Self::METHOD))
            .and(filters::params::<Value>())
            .and_then(
                move |response_builder: Builder, _params: Value| async move {
                    response_builder
                        .error(warp_json_rpc::Error::INVALID_PARAMS)
                        .map_err(|_| reject::reject())
                },
            );
        let with_missing_params = warp::path(RPC_API_PATH)
            .and(filters::json_rpc())
            .and(filters::method(Self::METHOD))
            .and_then(move |response_builder: Builder| async move {
                response_builder
                    .error(warp_json_rpc::Error::INVALID_PARAMS)
                    .map_err(|_| reject::reject())
            });
        with_valid_params
            .or(with_invalid_params)
            .unify()
            .or(with_missing_params)
            .unify()
            .boxed()
    }

    /// Handles the incoming RPC request.
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>>;
}

/// A JSON-RPC requiring the "params" field to be absent.
pub trait RpcWithoutParams {
    /// The JSON-RPC "method" name.
    const METHOD: &'static str;

    /// The JSON-RPC response's "result" type.
    type ResponseResult: Serialize
        + for<'de> Deserialize<'de>
        + JsonSchema
        + DocExample
        + Send
        + 'static;
}

/// A trait for creating a JSON-RPC filter where the request is not required to have "params".
pub(super) trait RpcWithoutParamsExt: RpcWithoutParams {
    /// Creates the warp filter for this particular RPC.
    fn create_filter<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
    ) -> BoxedFilter<(Response<Body>,)> {
        let with_no_params = warp::path(RPC_API_PATH)
            .and(filters::json_rpc())
            .and(filters::method(Self::METHOD))
            .and_then(move |response_builder: Builder| {
                Self::handle_request(effect_builder, response_builder, api_version)
                    .map_err(reject::custom)
            });
        let with_params = warp::path(RPC_API_PATH)
            .and(filters::json_rpc())
            .and(filters::method(Self::METHOD))
            .and(filters::params::<Value>())
            .and_then(
                move |response_builder: Builder, _params: Value| async move {
                    response_builder
                        .error(warp_json_rpc::Error::INVALID_PARAMS)
                        .map_err(|_| reject::reject())
                },
            );
        with_no_params.or(with_params).unify().boxed()
    }

    /// Handles the incoming RPC request.
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>>;
}

/// A JSON-RPC with the "params" field optional.
pub trait RpcWithOptionalParams {
    /// The JSON-RPC "method" name.
    const METHOD: &'static str;

    /// The JSON-RPC request's "params" type.  This will be passed to the handler wrapped in an
    /// `Option`.
    type OptionalRequestParams: Serialize
        + for<'de> Deserialize<'de>
        + JsonSchema
        + DocExample
        + Send
        + 'static;

    /// The JSON-RPC response's "result" type.
    type ResponseResult: Serialize
        + for<'de> Deserialize<'de>
        + JsonSchema
        + DocExample
        + Send
        + 'static;
}

/// A trait for creating a JSON-RPC filter where the request may optionally have "params".
pub(super) trait RpcWithOptionalParamsExt: RpcWithOptionalParams {
    /// Creates the warp filter for this particular RPC.
    fn create_filter<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
    ) -> BoxedFilter<(Response<Body>,)> {
        let with_params = warp::path(RPC_API_PATH)
            .and(filters::json_rpc())
            .and(filters::method(Self::METHOD))
            .and(filters::params::<Self::OptionalRequestParams>())
            .and_then(
                move |response_builder: Builder, params: Self::OptionalRequestParams| {
                    Self::handle_request(
                        effect_builder,
                        response_builder,
                        Some(params),
                        api_version,
                    )
                    .map_err(reject::custom)
                },
            );
        let with_invalid_params = warp::path(RPC_API_PATH)
            .and(filters::json_rpc())
            .and(filters::method(Self::METHOD))
            .and(filters::params::<Value>())
            .and_then(
                move |response_builder: Builder, _params: Value| async move {
                    response_builder
                        .error(warp_json_rpc::Error::INVALID_PARAMS)
                        .map_err(|_| reject::reject())
                },
            );
        let without_params = warp::path(RPC_API_PATH)
            .and(filters::json_rpc())
            .and(filters::method(Self::METHOD))
            .and_then(move |response_builder: Builder| {
                Self::handle_request(effect_builder, response_builder, None, api_version)
                    .map_err(reject::custom)
            });
        with_params
            .or(without_params)
            .unify()
            .or(with_invalid_params)
            .unify()
            .boxed()
    }

    /// Handles the incoming RPC request.
    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        maybe_params: Option<Self::OptionalRequestParams>,
        api_version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>>;
}
