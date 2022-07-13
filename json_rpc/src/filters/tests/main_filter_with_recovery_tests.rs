use std::sync::Arc;

use http::StatusCode;
use serde::{
    ser::{Error as _, Serializer},
    Deserialize, Serialize,
};
use serde_json::Value;
use warp::{filters::BoxedFilter, Filter, Reply};

use super::ResponseBodyOnRejection;
use crate::{
    filters::{handle_rejection, main_filter},
    Error, Params, RequestHandlersBuilder, ReservedErrorCode, Response,
};

const GET_GOOD_THING: &str = "get good thing";
const GET_BAD_THING: &str = "get bad thing";

#[derive(PartialEq, Eq, Serialize, Deserialize, Debug)]
struct GoodThing {
    good_thing: String,
}

/// A type which always errors when being serialized.
struct BadThing;

impl Serialize for BadThing {
    fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        Err(S::Error::custom("won't encode"))
    }
}

async fn get_good_thing(params: Option<Params>) -> Result<GoodThing, Error> {
    match params {
        Some(Params::Array(array)) => Ok(GoodThing {
            good_thing: array[0].as_str().unwrap().to_string(),
        }),
        _ => Err(Error::new(ReservedErrorCode::InvalidParams, "no params")),
    }
}

async fn get_bad_thing(_params: Option<Params>) -> Result<BadThing, Error> {
    Ok(BadThing)
}

async fn from_http_response(response: http::Response<hyper::Body>) -> Response {
    let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
    serde_json::from_slice(&body_bytes).unwrap()
}

fn main_filter_with_recovery() -> BoxedFilter<(impl Reply,)> {
    let mut handlers = RequestHandlersBuilder::new();
    handlers.register_handler(GET_GOOD_THING, Arc::new(get_good_thing));
    handlers.register_handler(GET_BAD_THING, Arc::new(get_bad_thing));
    let handlers = handlers.build();

    main_filter(handlers, false)
        .recover(handle_rejection)
        .boxed()
}

#[tokio::test]
async fn should_handle_valid_request() {
    let _ = env_logger::try_init();

    let filter = main_filter_with_recovery();

    // This should get handled by `fn get_good_thing` and return `Ok` as "params" is Some, causing a
    // Response::Success to be returned to the client.
    let http_response = warp::test::request()
        .body(r#"{"jsonrpc":"2.0","id":"a","method":"get good thing","params":["one"]}"#)
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(http_response.status(), StatusCode::OK);
    let rpc_response = from_http_response(http_response).await;
    assert_eq!(rpc_response.id(), "a");
    assert_eq!(
        rpc_response.result(),
        Some(GoodThing {
            good_thing: "one".to_string()
        })
    );
}

#[tokio::test]
async fn should_handle_valid_request_where_rpc_returns_error() {
    let _ = env_logger::try_init();

    let filter = main_filter_with_recovery();

    // This should get handled by `fn get_good_thing` and return `Err` as "params" is None, causing
    // a Response::Failure (invalid params) to be returned to the client.
    let http_response = warp::test::request()
        .body(r#"{"jsonrpc":"2.0","id":"a","method":"get good thing"}"#)
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(http_response.status(), StatusCode::OK);
    let rpc_response = from_http_response(http_response).await;
    assert_eq!(rpc_response.id(), "a");
    assert_eq!(
        rpc_response.error().unwrap(),
        &Error::new(ReservedErrorCode::InvalidParams, "no params")
    );
}

#[tokio::test]
async fn should_handle_valid_request_where_result_encoding_fails() {
    let _ = env_logger::try_init();

    let filter = main_filter_with_recovery();

    // This should get handled by `fn get_bad_thing` which returns a type which fails to encode,
    // causing a Response::Failure (internal error) to be returned to the client.
    let http_response = warp::test::request()
        .body(r#"{"jsonrpc":"2.0","id":"a","method":"get bad thing"}"#)
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(http_response.status(), StatusCode::OK);
    let rpc_response = from_http_response(http_response).await;
    assert_eq!(rpc_response.id(), "a");
    assert_eq!(
        rpc_response.error().unwrap(),
        &Error::new(
            ReservedErrorCode::InternalError,
            "failed to encode json-rpc response value: won't encode"
        )
    );
}

#[tokio::test]
async fn should_handle_request_for_method_not_registered() {
    let _ = env_logger::try_init();

    let filter = main_filter_with_recovery();

    // This should get handled by `filters::handle_body` and return Response::Failure (invalid
    // request) to the client as the ID has fractional parts.
    let http_response = warp::test::request()
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"not registered","params":["one"]}"#)
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(http_response.status(), StatusCode::OK);
    let rpc_response = from_http_response(http_response).await;
    assert_eq!(rpc_response.id(), 1);
    assert_eq!(
        rpc_response.error().unwrap(),
        &Error::new(
            ReservedErrorCode::MethodNotFound,
            "'not registered' is not a supported json-rpc method on this server"
        )
    );
}

#[tokio::test]
async fn should_handle_request_with_invalid_id() {
    let _ = env_logger::try_init();

    let filter = main_filter_with_recovery();

    // This should get handled by `filters::handle_body` and return Response::Failure (invalid
    // request) to the client as the ID has fractional parts.
    let http_response = warp::test::request()
        .body(r#"{"jsonrpc":"2.0","id":1.1,"method":"get good thing","params":["one"]}"#)
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(http_response.status(), StatusCode::OK);
    let rpc_response = from_http_response(http_response).await;
    assert_eq!(rpc_response.id(), &Value::Null);
    assert_eq!(
        rpc_response.error().unwrap(),
        &Error::new(
            ReservedErrorCode::InvalidRequest,
            "'id' must not contain fractional parts if it is a number"
        )
    );
}

#[tokio::test]
async fn should_handle_request_with_no_id() {
    let _ = env_logger::try_init();

    let filter = main_filter_with_recovery();

    // This should get handled by `filters::handle_body` and return no JSON-RPC response, only an
    // HTTP response (bad request) to the client as no ID was provided.
    let http_response = warp::test::request()
        .body(r#"{"jsonrpc":"2.0","method":"get good thing","params":["one"]}"#)
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(http_response.status(), StatusCode::BAD_REQUEST);
    let response_body = ResponseBodyOnRejection::from_response(http_response).await;
    assert_eq!(
        response_body.message,
        "The request is missing the 'id' field"
    );
}

#[tokio::test]
async fn should_handle_request_with_extra_field() {
    let _ = env_logger::try_init();

    let filter = main_filter_with_recovery();

    // This should get handled by `filters::handle_body` and return Response::Failure (invalid
    // request) to the client as the request has an extra field.
    let http_response = warp::test::request()
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"get good thing","params":[2],"extra":"field"}"#)
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(http_response.status(), StatusCode::OK);
    let rpc_response = from_http_response(http_response).await;
    assert_eq!(rpc_response.id(), 1);
    assert_eq!(
        rpc_response.error().unwrap(),
        &Error::new(
            ReservedErrorCode::InvalidRequest,
            "Unexpected field: 'extra'"
        )
    );
}

#[tokio::test]
async fn should_handle_malformed_request_with_valid_id() {
    let _ = env_logger::try_init();

    let filter = main_filter_with_recovery();

    // This should get handled by `filters::handle_body` and return Response::Failure (invalid
    // request) to the client, but with the ID included in the response as it was able to be parsed.
    let http_response = warp::test::request()
        .body(r#"{"jsonrpc":"2.0","id":1,"method":{"not":"a string"}}"#)
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(http_response.status(), StatusCode::OK);
    let rpc_response = from_http_response(http_response).await;
    assert_eq!(rpc_response.id(), 1);
    assert_eq!(
        rpc_response.error().unwrap(),
        &Error::new(
            ReservedErrorCode::InvalidRequest,
            "Expected 'method' to be a String"
        )
    );
}

#[tokio::test]
async fn should_handle_malformed_request_but_valid_json() {
    let _ = env_logger::try_init();

    let filter = main_filter_with_recovery();

    // This should get handled by `filters::handle_body` and return Response::Failure (invalid
    // request) to the client as it can't be parsed as a JSON-RPC request.
    let http_response = warp::test::request()
        .body(r#"{"a":1}"#)
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(http_response.status(), StatusCode::OK);
    let rpc_response = from_http_response(http_response).await;
    assert_eq!(rpc_response.id(), &Value::Null);
    assert_eq!(
        rpc_response.error().unwrap(),
        &Error::new(ReservedErrorCode::InvalidRequest, "Missing 'jsonrpc' field")
    );
}

#[tokio::test]
async fn should_handle_invalid_json() {
    let _ = env_logger::try_init();

    let filter = main_filter_with_recovery();

    // This should get handled by `filters::handle_body` and return Response::Failure (parse error)
    // to the client as it cannot be parsed as JSON.
    let http_response = warp::test::request()
        .body(r#"a"#)
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(http_response.status(), StatusCode::OK);
    let rpc_response = from_http_response(http_response).await;
    assert_eq!(rpc_response.id(), &Value::Null);
    assert_eq!(
        rpc_response.error().unwrap(),
        &Error::new(
            ReservedErrorCode::ParseError,
            "expected value at line 1 column 1"
        )
    );
}
