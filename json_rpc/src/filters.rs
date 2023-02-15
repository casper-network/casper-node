//! Warp filters which can be combined to provide JSON-RPC endpoints.
//!
//! Generally these lower-level filters will not need to be explicitly called.  Instead,
//! [`casper_json_rpc::route()`](crate::route) should be sufficient.

#[cfg(test)]
mod tests;

use bytes::Bytes;
use http::{header::CONTENT_TYPE, HeaderMap, StatusCode};
use serde_json::{json, Map, Value};
use tracing::{debug, trace, warn};
use warp::{
    body,
    filters::BoxedFilter,
    reject::{self, Rejection},
    reply::{self, WithStatus},
    Filter,
};

use crate::{
    error::{Error, ReservedErrorCode},
    rejections::{BodyTooLarge, MissingContentTypeHeader, MissingId, UnsupportedMediaType},
    request::{ErrorOrRejection, Request},
    request_handlers::RequestHandlers,
    response::Response,
};

const CONTENT_TYPE_VALUE: &str = "application/json";

/// Returns a boxed warp filter which handles the initial setup.
///
/// This includes:
///   * setting the full path
///   * setting the method to POST
///   * ensuring the "content-type" header exists and is set to "application/json"
///   * ensuring the body has at most `max_body_bytes` bytes
pub fn base_filter<P: AsRef<str>>(path: P, max_body_bytes: u32) -> BoxedFilter<()> {
    let path = path.as_ref().to_string();
    warp::path::path(path)
        .and(warp::path::end())
        .and(warp::filters::method::post())
        .and(
            warp::filters::header::headers_cloned().and_then(|headers: HeaderMap| async move {
                for (name, value) in headers.iter() {
                    if name.as_str() == CONTENT_TYPE.as_str() {
                        if value
                            .as_bytes()
                            .eq_ignore_ascii_case(CONTENT_TYPE_VALUE.as_bytes())
                        {
                            return Ok(());
                        } else {
                            trace!(content_type = ?value.to_str(), "invalid {}", CONTENT_TYPE);
                            return Err(reject::custom(UnsupportedMediaType));
                        }
                    }
                }
                trace!("missing {}", CONTENT_TYPE);
                Err(reject::custom(MissingContentTypeHeader))
            }),
        )
        .untuple_one()
        .and(body::content_length_limit(max_body_bytes as u64).or_else(
            move |_rejection| async move { Err(reject::custom(BodyTooLarge(max_body_bytes))) },
        ))
        .boxed()
}

/// Handles parsing a JSON-RPC request from the given HTTP body, executing it using the appropriate
/// handler, and providing a JSON-RPC response (which could be a success or failure).
///
/// Returns an `Err(Rejection)` only if the request is a Notification as per the JSON-RPC
/// specification, i.e. the request doesn't contain an "id" field.  In this case, no JSON-RPC
/// response is sent to the client.
///
/// If `allow_unknown_fields` is `false`, requests with unknown fields will cause the server to
/// respond with an error.
async fn handle_body(
    body: Bytes,
    handlers: RequestHandlers,
    allow_unknown_fields: bool,
) -> Result<Response, Rejection> {
    let response = match serde_json::from_slice::<Map<String, Value>>(&body) {
        Ok(unvalidated_request) => match Request::new(unvalidated_request, allow_unknown_fields) {
            Ok(request) => handlers.handle_request(request).await,
            Err(ErrorOrRejection::Error { id, error }) => {
                debug!(?error, "got an invalid request");
                Response::new_failure(id, error)
            }
            Err(ErrorOrRejection::Rejection(rejection)) => {
                debug!(?rejection, "rejecting an invalid request");
                return Err(rejection);
            }
        },
        Err(error) => {
            debug!(%error, "got bad json");
            let error = Error::new(ReservedErrorCode::ParseError, error.to_string());
            Response::new_failure(Value::Null, error)
        }
    };
    Ok(response)
}

/// Returns a boxed warp filter which handles parsing a JSON-RPC request from the given HTTP body,
/// executing it using the appropriate handler, and providing a reply.
///
/// The reply will normally be built from a JSON-RPC response (which could be a success or failure).
///
/// However, the reply could be built from a [`Rejection`] if the request is a Notification as per
/// the JSON-RPC specification, i.e. the request doesn't contain an "id" field.  In this case, no
/// JSON-RPC response is sent to the client, only an HTTP response.
///
/// If `allow_unknown_fields` is `false`, requests with unknown fields will cause the server to
/// respond with an error.
pub fn main_filter(
    handlers: RequestHandlers,
    allow_unknown_fields: bool,
) -> BoxedFilter<(WithStatus<reply::Json>,)> {
    body::bytes()
        .and_then(move |body| {
            let handlers = handlers.clone();
            async move { handle_body(body, handlers, allow_unknown_fields).await }
        })
        .map(|response| reply::with_status(reply::json(&response), StatusCode::OK))
        .boxed()
}

/// Handler for rejections where no JSON-RPC response is sent, but an HTTP response is required.
///
/// The HTTP response body will be a JSON object of the form:
/// ```json
/// { "message": <String> }
/// ```
pub async fn handle_rejection(error: Rejection) -> Result<WithStatus<reply::Json>, Rejection> {
    let code;
    let message;

    if let Some(rejection) = error.find::<UnsupportedMediaType>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::UNSUPPORTED_MEDIA_TYPE;
    } else if let Some(rejection) = error.find::<MissingContentTypeHeader>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::BAD_REQUEST;
    } else if let Some(rejection) = error.find::<MissingId>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::BAD_REQUEST;
    } else if let Some(rejection) = error.find::<BodyTooLarge>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::PAYLOAD_TOO_LARGE;
    } else if error.is_not_found() {
        trace!("{:?}", error);
        message = "Path not found".to_string();
        code = StatusCode::NOT_FOUND;
    } else if let Some(rejection) = error.find::<reject::MethodNotAllowed>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::METHOD_NOT_ALLOWED;
    } else if let Some(rejection) = error.find::<reject::InvalidHeader>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::BAD_REQUEST;
    } else if let Some(rejection) = error.find::<reject::MissingHeader>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::BAD_REQUEST;
    } else if let Some(rejection) = error.find::<reject::InvalidQuery>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::BAD_REQUEST;
    } else if let Some(rejection) = error.find::<reject::MissingCookie>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::BAD_REQUEST;
    } else if let Some(rejection) = error.find::<reject::LengthRequired>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::LENGTH_REQUIRED;
    } else if let Some(rejection) = error.find::<reject::PayloadTooLarge>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::PAYLOAD_TOO_LARGE;
    } else if let Some(rejection) = error.find::<reject::UnsupportedMediaType>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::UNSUPPORTED_MEDIA_TYPE;
    } else if let Some(rejection) = error.find::<warp::filters::cors::CorsForbidden>() {
        trace!("{:?}", rejection);
        message = rejection.to_string();
        code = StatusCode::FORBIDDEN;
    } else {
        // We should handle all rejection types before this.
        warn!(?error, "unhandled warp rejection in json-rpc server");
        message = format!("Internal server error: unhandled rejection: {:?}", error);
        code = StatusCode::INTERNAL_SERVER_ERROR;
    }

    Ok(reply::with_status(
        reply::json(&json!({ "message": message })),
        code,
    ))
}
