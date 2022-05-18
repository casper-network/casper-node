use http::StatusCode;
use warp::{filters::BoxedFilter, reply, test::RequestBuilder, Filter, Reply};

use super::ResponseBodyOnRejection;
use crate::filters::{base_filter, handle_rejection, CONTENT_TYPE_VALUE};

const PATH: &str = "rpc";
const MAX_BODY_BYTES: u32 = 10;

fn base_filter_with_recovery() -> BoxedFilter<(impl Reply,)> {
    base_filter(PATH, MAX_BODY_BYTES)
        .map(reply) // return an empty body on success
        .with(warp::cors().allow_origin("http://a.com"))
        .recover(handle_rejection) // or convert a rejection to JSON-encoded `ResponseBody`
        .boxed()
}

fn valid_base_filter_request_builder() -> RequestBuilder {
    warp::test::request()
        .path(&format!("/{}", PATH))
        .header("content-type", CONTENT_TYPE_VALUE)
        .method("POST")
        .body([0_u8; MAX_BODY_BYTES as usize])
}

#[tokio::test]
async fn should_accept_valid_request() {
    let _ = env_logger::try_init();

    let filter = base_filter_with_recovery();

    let response = valid_base_filter_request_builder()
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
    assert!(body_bytes.is_empty());
}

#[tokio::test]
async fn should_reject_invalid_path() {
    async fn test_with_invalid_path(path: &str) {
        let filter = base_filter_with_recovery();

        let response = valid_base_filter_request_builder()
            .path(path)
            .filter(&filter)
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response_body = ResponseBodyOnRejection::from_response(response).await;
        assert_eq!(response_body.message, "Path not found");
    }

    let _ = env_logger::try_init();

    // A root path.
    test_with_invalid_path("/").await;

    // A path which doesn't match the server's.
    test_with_invalid_path("/not_the_right_path").await;

    // A path which extends the server's
    test_with_invalid_path(&format!("/{0}/{0}", PATH)).await;
}

#[tokio::test]
async fn should_reject_unsupported_http_method() {
    async fn test_with_unsupported_method(method: &'static str) {
        let filter = base_filter_with_recovery();

        let response = valid_base_filter_request_builder()
            .method(method)
            .filter(&filter)
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let response_body = ResponseBodyOnRejection::from_response(response).await;
        assert_eq!(response_body.message, "HTTP method not allowed");
    }

    let _ = env_logger::try_init();

    test_with_unsupported_method("GET").await;
    test_with_unsupported_method("PUT").await;
    test_with_unsupported_method("DELETE").await;
    test_with_unsupported_method("HEAD").await;
    test_with_unsupported_method("OPTIONS").await;
    test_with_unsupported_method("CONNECT").await;
    test_with_unsupported_method("PATCH").await;
    test_with_unsupported_method("TRACE").await;
    test_with_unsupported_method("a").await;
}

#[tokio::test]
async fn should_reject_missing_content_type_header() {
    let _ = env_logger::try_init();

    let filter = base_filter_with_recovery();

    let response = warp::test::request()
        .path(&format!("/{}", PATH))
        .method("POST")
        .body("")
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response_body = ResponseBodyOnRejection::from_response(response).await;
    assert_eq!(
        response_body.message,
        "The request's content-type is not set"
    );
}

#[tokio::test]
async fn should_reject_invalid_content_type() {
    async fn test_invalid_content_type(value: &'static str) {
        let filter = base_filter_with_recovery();

        let response = valid_base_filter_request_builder()
            .header("content-type", value)
            .filter(&filter)
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let response_body = ResponseBodyOnRejection::from_response(response).await;
        assert_eq!(
            response_body.message,
            "The request's content-type is not supported"
        );
    }

    let _ = env_logger::try_init();

    test_invalid_content_type("text/html").await;
    test_invalid_content_type("multipart/form-data").await;
    test_invalid_content_type("a").await;
    test_invalid_content_type("").await;
}

#[tokio::test]
async fn should_reject_large_body() {
    let _ = env_logger::try_init();

    let filter = base_filter_with_recovery();

    let response = valid_base_filter_request_builder()
        .body([0_u8; MAX_BODY_BYTES as usize + 1])
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let response_body = ResponseBodyOnRejection::from_response(response).await;
    assert_eq!(
        response_body.message,
        "The request payload exceeds the maximum allowed of 10 bytes"
    );
}

#[tokio::test]
async fn should_reject_cors() {
    let _ = env_logger::try_init();

    let filter = base_filter_with_recovery();

    let response = valid_base_filter_request_builder()
        .header("Origin", "http://b.com")
        .filter(&filter)
        .await
        .unwrap()
        .into_response();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response_body = ResponseBodyOnRejection::from_response(response).await;
    assert_eq!(
        response_body.message,
        "CORS request forbidden: origin not allowed"
    );
}

#[tokio::test]
async fn should_handle_any_case_content_type() {
    async fn test_content_type(key: &'static str, value: &'static str) {
        let filter = base_filter_with_recovery();

        let response = valid_base_filter_request_builder()
            .header(key, value)
            .filter(&filter)
            .await
            .unwrap()
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
        assert!(body_bytes.is_empty());
    }

    let _ = env_logger::try_init();

    test_content_type("Content-Type", "application/json").await;
    test_content_type("Content-Type", "Application/JSON").await;
    test_content_type("content-type", "application/json").await;
    test_content_type("content-type", "Application/JSON").await;
    test_content_type("CONTENT-TYPE", "APPLICATION/JSON").await;
    test_content_type("CoNtEnT-tYpE", "ApPliCaTiOn/JsOn").await;
}
