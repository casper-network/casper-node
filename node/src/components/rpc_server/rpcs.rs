//! The set of JSON-RPCs which the API server handles.
//!
//! See <https://github.com/CasperLabs/ceps/blob/master/text/0009-client-api.md#rpcs> for info.

pub mod account;
pub mod chain;
mod common;
pub mod docs;
mod error_code;
pub mod info;
pub mod state;

use std::{str, sync::Arc};

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use casper_json_rpc::{Error, RequestHandlersBuilder, ReservedErrorCode};
use casper_types::ProtocolVersion;

use super::{ReactorEventT, RpcRequest};
use crate::effect::EffectBuilder;
pub use common::ErrorData;
use docs::DocExample;
pub use error_code::ErrorCode;

/// A JSON-RPC requiring the "params" field to be present.
#[async_trait]
pub(super) trait RpcWithParams {
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
        + PartialEq
        + JsonSchema
        + DocExample
        + Send
        + 'static;

    /// Tries to parse the incoming JSON-RPC request's "params" field as `RequestParams`.
    fn try_parse_params(maybe_params: Option<Value>) -> Result<Self::RequestParams, Error> {
        let params = maybe_params.unwrap_or(Value::Null);
        if params.is_null() {
            return Err(Error::new(
                ReservedErrorCode::InvalidParams,
                "Missing or null 'params' field",
            ));
        }
        serde_json::from_value::<Self::RequestParams>(params).map_err(|error| {
            Error::new(
                ReservedErrorCode::InvalidParams,
                format!("Failed to parse 'params' field: {}", error),
            )
        })
    }

    /// Registers this RPC as the handler for JSON-RPC requests whose "method" field is the same as
    /// `Self::METHOD`.
    fn register_as_handler<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        handlers_builder: &mut RequestHandlersBuilder,
    ) {
        let handler = move |maybe_params| async move {
            let params = Self::try_parse_params(maybe_params)?;
            Self::do_handle_request(effect_builder, api_version, params).await
        };
        handlers_builder.register_handler(Self::METHOD, Arc::new(handler))
    }

    /// Tries to parse the params, and on success, returns the doc example, regardless of the value
    /// of the parsed params.
    #[cfg(test)]
    fn register_as_test_handler(handlers_builder: &mut RequestHandlersBuilder) {
        let handler = move |maybe_params| async move {
            let _params = Self::try_parse_params(maybe_params)?;
            Ok(Self::ResponseResult::doc_example())
        };
        handlers_builder.register_handler(Self::METHOD, Arc::new(handler))
    }

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error>;
}

/// A JSON-RPC requiring the "params" field to be absent.
#[async_trait]
pub(super) trait RpcWithoutParams {
    /// The JSON-RPC "method" name.
    const METHOD: &'static str;

    /// The JSON-RPC response's "result" type.
    type ResponseResult: Serialize
        + for<'de> Deserialize<'de>
        + PartialEq
        + JsonSchema
        + DocExample
        + Send
        + 'static;

    /// Returns an error if the incoming JSON-RPC request's "params" field is not `None` or `Null`.
    fn check_no_params(maybe_params: Option<Value>) -> Result<(), Error> {
        if !maybe_params.unwrap_or(Value::Null).is_null() {
            return Err(Error::new(
                ReservedErrorCode::InvalidParams,
                "'params' field should be null or absent",
            ));
        }
        Ok(())
    }

    /// Registers this RPC as the handler for JSON-RPC requests whose "method" field is the same as
    /// `Self::METHOD`.
    fn register_as_handler<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        handlers_builder: &mut RequestHandlersBuilder,
    ) {
        let handler = move |maybe_params| async move {
            Self::check_no_params(maybe_params)?;
            Self::do_handle_request(effect_builder, api_version).await
        };
        handlers_builder.register_handler(Self::METHOD, Arc::new(handler))
    }

    /// Checks the params, and on success, returns the doc example.
    #[cfg(test)]
    fn register_as_test_handler(handlers_builder: &mut RequestHandlersBuilder) {
        let handler = move |maybe_params| async move {
            Self::check_no_params(maybe_params)?;
            Ok(Self::ResponseResult::doc_example())
        };
        handlers_builder.register_handler(Self::METHOD, Arc::new(handler))
    }

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
    ) -> Result<Self::ResponseResult, Error>;
}

/// A JSON-RPC where the "params" field is optional.
#[async_trait]
pub(super) trait RpcWithOptionalParams {
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
        + PartialEq
        + JsonSchema
        + DocExample
        + Send
        + 'static;

    /// Tries to parse the incoming JSON-RPC request's "params" field as
    /// `Option<OptionalRequestParams>`.
    fn try_parse_params(
        maybe_params: Option<Value>,
    ) -> Result<Option<Self::OptionalRequestParams>, Error> {
        serde_json::from_value::<Option<Self::OptionalRequestParams>>(
            maybe_params.unwrap_or(Value::Null),
        )
        .map_err(|error| {
            Error::new(
                ReservedErrorCode::InvalidParams,
                format!("Failed to parse 'params' field: {}", error),
            )
        })
    }

    /// Registers this RPC as the handler for JSON-RPC requests whose "method" field is the same as
    /// `Self::METHOD`.
    fn register_as_handler<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        handlers_builder: &mut RequestHandlersBuilder,
    ) {
        let handler = move |maybe_params| async move {
            let params = Self::try_parse_params(maybe_params)?;
            Self::do_handle_request(effect_builder, api_version, params).await
        };
        handlers_builder.register_handler(Self::METHOD, Arc::new(handler))
    }

    /// Tries to parse the params, and on success, returns the doc example, regardless of the value
    /// of the parsed params.
    #[cfg(test)]
    fn register_as_test_handler(handlers_builder: &mut RequestHandlersBuilder) {
        let handler = move |maybe_params| async move {
            let _params = Self::try_parse_params(maybe_params)?;
            Ok(Self::ResponseResult::doc_example())
        };
        handlers_builder.register_handler(Self::METHOD, Arc::new(handler))
    }

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Option<Self::OptionalRequestParams>,
    ) -> Result<Self::ResponseResult, Error>;
}

#[cfg(test)]
mod tests {
    use http::StatusCode;
    use warp::{filters::BoxedFilter, Filter, Reply};

    use casper_json_rpc::{filters, Response};

    use super::*;
    use crate::types::DeployHash;

    async fn send_request(
        method: &str,
        maybe_params: Option<&str>,
        filter: &BoxedFilter<(impl Reply + 'static,)>,
    ) -> Response {
        let mut body = format!(r#"{{"jsonrpc":"2.0","id":"a","method":"{}""#, method);
        match maybe_params {
            Some(params) => body += &format!(r#","params":{}}}"#, params),
            None => body += "}",
        }

        let http_response = warp::test::request()
            .body(body)
            .filter(filter)
            .await
            .unwrap()
            .into_response();

        assert_eq!(http_response.status(), StatusCode::OK);
        let body_bytes = hyper::body::to_bytes(http_response.into_body())
            .await
            .unwrap();
        serde_json::from_slice(&body_bytes).unwrap()
    }

    mod rpc_with_params {
        use super::*;
        use crate::components::rpc_server::rpcs::info::{
            GetDeploy, GetDeployParams, GetDeployResult,
        };

        fn main_filter_with_recovery() -> BoxedFilter<(impl Reply,)> {
            let mut handlers = RequestHandlersBuilder::new();
            GetDeploy::register_as_test_handler(&mut handlers);
            let handlers = handlers.build();

            filters::main_filter(handlers)
                .recover(filters::handle_rejection)
                .boxed()
        }

        #[tokio::test]
        async fn should_parse_params() {
            let filter = main_filter_with_recovery();

            let params = serde_json::to_string(&GetDeployParams {
                deploy_hash: DeployHash::default(),
                finalized_approvals: false,
            })
            .unwrap();
            let params = Some(params.as_str());
            let rpc_response = send_request(GetDeploy::METHOD, params, &filter).await;
            assert_eq!(
                rpc_response.result().as_ref(),
                Some(GetDeployResult::doc_example())
            );
        }

        #[tokio::test]
        async fn should_return_error_if_missing_params() {
            let filter = main_filter_with_recovery();

            let rpc_response = send_request(GetDeploy::METHOD, None, &filter).await;
            assert_eq!(
                rpc_response.error().unwrap(),
                &Error::new(
                    ReservedErrorCode::InvalidParams,
                    "Missing or null 'params' field"
                )
            );

            let rpc_response = send_request(GetDeploy::METHOD, Some("null"), &filter).await;
            assert_eq!(
                rpc_response.error().unwrap(),
                &Error::new(
                    ReservedErrorCode::InvalidParams,
                    "Missing or null 'params' field"
                )
            );
        }

        #[tokio::test]
        async fn should_return_error_on_failure_to_parse_params() {
            let filter = main_filter_with_recovery();

            let rpc_response = send_request(GetDeploy::METHOD, Some("3"), &filter).await;
            assert_eq!(
                rpc_response.error().unwrap(),
                &Error::new(
                    ReservedErrorCode::InvalidParams,
                    "Failed to parse 'params' field: invalid type: integer `3`, expected struct \
                    GetDeployParams"
                )
            );
        }
    }

    mod rpc_without_params {
        use super::*;
        use crate::components::rpc_server::rpcs::info::{GetPeers, GetPeersResult};

        fn main_filter_with_recovery() -> BoxedFilter<(impl Reply,)> {
            let mut handlers = RequestHandlersBuilder::new();
            GetPeers::register_as_test_handler(&mut handlers);
            let handlers = handlers.build();

            filters::main_filter(handlers)
                .recover(filters::handle_rejection)
                .boxed()
        }

        #[tokio::test]
        async fn should_check_no_params() {
            let filter = main_filter_with_recovery();

            let rpc_response = send_request(GetPeers::METHOD, None, &filter).await;
            assert_eq!(
                rpc_response.result().as_ref(),
                Some(GetPeersResult::doc_example())
            );

            let rpc_response = send_request(GetPeers::METHOD, Some("null"), &filter).await;
            assert_eq!(
                rpc_response.result().as_ref(),
                Some(GetPeersResult::doc_example())
            );
        }

        #[tokio::test]
        async fn should_return_error_if_params_not_null() {
            let filter = main_filter_with_recovery();

            let rpc_response = send_request(GetPeers::METHOD, Some("3"), &filter).await;
            assert_eq!(
                rpc_response.error().unwrap(),
                &Error::new(
                    ReservedErrorCode::InvalidParams,
                    "'params' field should be null or absent"
                )
            );
        }
    }

    mod rpc_with_optional_params {
        use super::*;
        use crate::components::rpc_server::rpcs::chain::{
            BlockIdentifier, GetBlock, GetBlockParams, GetBlockResult,
        };

        fn main_filter_with_recovery() -> BoxedFilter<(impl Reply,)> {
            let mut handlers = RequestHandlersBuilder::new();
            GetBlock::register_as_test_handler(&mut handlers);
            let handlers = handlers.build();

            filters::main_filter(handlers)
                .recover(filters::handle_rejection)
                .boxed()
        }

        #[tokio::test]
        async fn should_parse_without_params() {
            let filter = main_filter_with_recovery();

            let rpc_response = send_request(GetBlock::METHOD, None, &filter).await;
            assert_eq!(
                rpc_response.result().as_ref(),
                Some(GetBlockResult::doc_example())
            );

            let rpc_response = send_request(GetBlock::METHOD, Some("null"), &filter).await;
            assert_eq!(
                rpc_response.result().as_ref(),
                Some(GetBlockResult::doc_example())
            );
        }

        #[tokio::test]
        async fn should_parse_with_params() {
            let filter = main_filter_with_recovery();

            let params = serde_json::to_string(&GetBlockParams {
                block_identifier: BlockIdentifier::Height(1),
            })
            .unwrap();
            let params = Some(params.as_str());

            let rpc_response = send_request(GetBlock::METHOD, params, &filter).await;
            assert_eq!(
                rpc_response.result().as_ref(),
                Some(GetBlockResult::doc_example())
            );
        }

        #[tokio::test]
        async fn should_return_error_on_failure_to_parse_params() {
            let filter = main_filter_with_recovery();

            let rpc_response = send_request(GetBlock::METHOD, Some("3"), &filter).await;
            assert_eq!(
                rpc_response.error().unwrap(),
                &Error::new(
                    ReservedErrorCode::InvalidParams,
                    "Failed to parse 'params' field: invalid type: integer `3`, expected struct \
                    GetBlockParams"
                )
            );
        }
    }
}
