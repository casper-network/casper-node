//! # casper-json-rpc
//!
//! A library suitable for use as the framework for a JSON-RPC server.
//!
//! # Usage
//!
//! Normally usage will involve two steps:
//!   * construct a set of request handlers using a [`RequestHandlersBuilder`]
//!   * call [`casper_json_rpc::route`](route) to construct a boxed warp filter ready to be passed
//!     to [`warp::service`](https://docs.rs/warp/latest/warp/fn.service.html) for example
//!
//! # Example
//!
//! ```no_run
//! use casper_json_rpc::{Error, RequestHandlersBuilder};
//! use serde_json::Value;
//! use std::{convert::Infallible, sync::Arc};
//!
//! # #[allow(unused)]
//! async fn get(params: Option<Value>) -> Result<String, Error> {
//!     // * parse params or return `ReservedErrorCode::InvalidParams` error
//!     // * handle request and return result
//!     Ok("got it".to_string())
//! }
//!
//! # #[allow(unused)]
//! async fn put(params: Option<Value>, other_input: &str) -> Result<String, Error> {
//!     Ok(other_input.to_string())
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     // Register handlers for methods "get" and "put".
//!     let mut handlers = RequestHandlersBuilder::new();
//!     handlers.register_handler("get", Arc::new(get));
//!     let put_handler = move |params| async move { put(params, "other input").await };
//!     handlers.register_handler("put", Arc::new(put_handler));
//!     let handlers = handlers.build();
//!
//!     // Get the new route.
//!     let path = "rpc";
//!     let max_body_bytes = 1024;
//!     let route = casper_json_rpc::route(path, max_body_bytes, handlers);
//!
//!     // Convert it into a `Service` and run it.
//!     let make_svc = hyper::service::make_service_fn(move |_| {
//!         let svc = warp::service(route.clone());
//!         async move { Ok::<_, Infallible>(svc.clone()) }
//!     });
//!
//!     hyper::Server::bind(&([127, 0, 0, 1], 3030).into())
//!         .serve(make_svc)
//!         .await
//!         .unwrap();
//! }
//! ```
//!
//! # Errors
//!
//! To return a JSON-RPC response indicating an error, use [`Error::new`].  Most error conditions
//! which require returning a reserved error are already handled in the provided warp filters.  The
//! only exception is [`ReservedErrorCode::InvalidParams`] which should be returned by any RPC
//! handler which deems the provided `params: Option<Value>` to be invalid for any reason.
//!
//! Generally a set of custom error codes should be provided.  These should all implement
//! [`ErrorCodeT`].

#![doc(html_root_url = "https://docs.rs/casper-json-rpc/0.1.0")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/casper-network/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/casper-network/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(deny(warnings)))
)]
#![warn(
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unused_qualifications
)]

mod error;
pub mod filters;
mod rejections;
mod request;
mod request_handlers;
mod response;

use http::{header::CONTENT_TYPE, Method};
use warp::{filters::BoxedFilter, Filter, Reply};

pub use error::{Error, ErrorCodeT, ReservedErrorCode};
pub use request_handlers::{RequestHandlers, RequestHandlersBuilder};
pub use response::Response;

const JSON_RPC_VERSION: &str = "2.0";

/// Constructs a set of warp filters suitable for use in a JSON-RPC server.
///
/// `path` specifies the exact HTTP path for JSON-RPC requests, e.g. "rpc" will match requests on
/// exactly "/rpc", and not "/rpc/other".
///
/// `max_body_bytes` sets an upper limit for the number of bytes in the HTTP request body.  For
/// further details, see
/// [`warp::filters::body::content_length_limit`](https://docs.rs/warp/latest/warp/filters/body/fn.content_length_limit.html).
///
/// `handlers` is the map of functions to which incoming requests will be dispatched.  These are
/// keyed by the JSON-RPC request's "method".
///
/// Note that this is a convenience function combining the lower-level functions in [`filters`]
/// along with [a warp CORS filter](https://docs.rs/warp/latest/warp/filters/cors/index.html) which
///   * allows any origin
///   * allows "content-type" as a header
///   * allows the method "POST"
///
/// For further details, see the docs for the [`filters`] functions.
pub fn route<P: AsRef<str>>(
    path: P,
    max_body_bytes: u32,
    handlers: RequestHandlers,
) -> BoxedFilter<(impl Reply,)> {
    filters::base_filter(path, max_body_bytes)
        .and(filters::main_filter(handlers))
        .recover(filters::handle_rejection)
        .with(
            warp::cors()
                .allow_any_origin()
                .allow_header(CONTENT_TYPE)
                .allow_method(Method::POST),
        )
        .boxed()
}
