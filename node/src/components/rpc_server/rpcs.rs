//! The set of JSON-RPCs which the API server handles.
//!
//! See <https://github.com/CasperLabs/ceps/blob/master/text/0009-client-api.md#rpcs> for info.

pub mod account;
pub mod chain;
mod common;
pub mod docs;
mod error_code;
pub mod info;
pub mod speculative_exec;
pub mod state;

use std::{convert::Infallible, str, sync::Arc, time::Duration};

use async_trait::async_trait;
use http::header::ACCEPT_ENCODING;
use hyper::server::{conn::AddrIncoming, Builder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;
use tower::ServiceBuilder;
use tracing::info;
use warp::Filter;

use casper_json_rpc::{Error, Params, RequestHandlers, RequestHandlersBuilder, ReservedErrorCode};
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
    fn try_parse_params(maybe_params: Option<Params>) -> Result<Self::RequestParams, Error> {
        let params = match maybe_params {
            Some(params) => Value::from(params),
            None => {
                return Err(Error::new(
                    ReservedErrorCode::InvalidParams,
                    "Missing 'params' field",
                ))
            }
        };
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

    /// Returns an error if the incoming JSON-RPC request's "params" field is not `None` or an empty
    /// Array or Object.
    fn check_no_params(maybe_params: Option<Params>) -> Result<(), Error> {
        if !maybe_params.unwrap_or_default().is_empty() {
            return Err(Error::new(
                ReservedErrorCode::InvalidParams,
                "'params' field should be an empty Array '[]', an empty Object '{}' or absent",
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
///
/// Note that "params" being an empty JSON Array or empty JSON Object is treated the same as if
/// the "params" field is absent - i.e. it represents the `None` case.
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
        maybe_params: Option<Params>,
    ) -> Result<Option<Self::OptionalRequestParams>, Error> {
        let params = match maybe_params {
            Some(params) => {
                if params.is_empty() {
                    Value::Null
                } else {
                    Value::from(params)
                }
            }
            None => Value::Null,
        };
        serde_json::from_value::<Option<Self::OptionalRequestParams>>(params).map_err(|error| {
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

/// Start JSON RPC server in a background.
pub(super) async fn run(
    builder: Builder<AddrIncoming>,
    handlers: RequestHandlers,
    qps_limit: u64,
    max_body_bytes: u32,
    api_path: &'static str,
    server_name: &'static str,
) {
    let make_svc = hyper::service::make_service_fn(move |_| {
        let service_routes = casper_json_rpc::route(api_path, max_body_bytes, handlers.clone());

        // Supports content negotiation for gzip responses. This is an interim fix until
        // https://github.com/seanmonstar/warp/pull/513 moves forward.
        let service_routes_gzip = warp::header::exact(ACCEPT_ENCODING.as_str(), "gzip")
            .and(service_routes.clone())
            .with(warp::compression::gzip());

        let service = warp::service(service_routes_gzip.or(service_routes));
        async move { Ok::<_, Infallible>(service.clone()) }
    });

    let make_svc = ServiceBuilder::new()
        .rate_limit(qps_limit, Duration::from_secs(1))
        .service(make_svc);

    let server = builder.serve(make_svc);
    info!(address = %server.local_addr(), "started {} server", server_name);

    let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();
    let server_with_shutdown = server.with_graceful_shutdown(async {
        shutdown_receiver.await.ok();
    });

    let _ = tokio::spawn(server_with_shutdown).await;
    let _ = shutdown_sender.send(());
    info!("{} server shut down", server_name);
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
                &Error::new(ReservedErrorCode::InvalidParams, "Missing 'params' field")
            );

            let rpc_response = send_request(GetDeploy::METHOD, Some("[]"), &filter).await;
            assert_eq!(
                rpc_response.error().unwrap(),
                &Error::new(
                    ReservedErrorCode::InvalidParams,
                    "Failed to parse 'params' field: invalid length 0, expected struct \
                    GetDeployParams with 2 elements"
                )
            );
        }

        #[tokio::test]
        async fn should_return_error_on_failure_to_parse_params() {
            let filter = main_filter_with_recovery();

            let rpc_response = send_request(GetDeploy::METHOD, Some("[3]"), &filter).await;
            assert_eq!(
                rpc_response.error().unwrap(),
                &Error::new(
                    ReservedErrorCode::InvalidParams,
                    "Failed to parse 'params' field: invalid type: integer `3`, expected a string"
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

            let rpc_response = send_request(GetPeers::METHOD, Some("[]"), &filter).await;
            assert_eq!(
                rpc_response.result().as_ref(),
                Some(GetPeersResult::doc_example())
            );

            let rpc_response = send_request(GetPeers::METHOD, Some("{}"), &filter).await;
            assert_eq!(
                rpc_response.result().as_ref(),
                Some(GetPeersResult::doc_example())
            );
        }

        #[tokio::test]
        async fn should_return_error_if_params_not_empty() {
            let filter = main_filter_with_recovery();

            let rpc_response = send_request(GetPeers::METHOD, Some("[3]"), &filter).await;
            assert_eq!(
                rpc_response.error().unwrap(),
                &Error::new(
                    ReservedErrorCode::InvalidParams,
                    "'params' field should be an empty Array '[]', an empty Object '{}' or absent"
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

            let rpc_response = send_request(GetBlock::METHOD, Some("[]"), &filter).await;
            assert_eq!(
                rpc_response.result().as_ref(),
                Some(GetBlockResult::doc_example())
            );

            let rpc_response = send_request(GetBlock::METHOD, Some("{}"), &filter).await;
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

            let rpc_response = send_request(GetBlock::METHOD, Some(r#"["a"]"#), &filter).await;
            assert_eq!(
                rpc_response.error().unwrap(),
                &Error::new(
                    ReservedErrorCode::InvalidParams,
                    "Failed to parse 'params' field: unknown variant `a`, expected `Hash` or \
                    `Height`"
                )
            );
        }
    }
}
