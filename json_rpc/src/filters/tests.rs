mod base_filter_with_recovery_tests;
mod main_filter_with_recovery_tests;

use serde::Deserialize;

/// The HTTP response body returned in the event of a warp rejection.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseBodyOnRejection {
    message: String,
}

impl ResponseBodyOnRejection {
    async fn from_response(response: http::Response<hyper::Body>) -> Self {
        let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
        serde_json::from_slice(&body_bytes).unwrap()
    }
}
