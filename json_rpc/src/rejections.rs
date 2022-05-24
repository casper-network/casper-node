//! These types are used to allow a given warp filter to reject a request.  The rejections are
//! handled in a subsequent function, where they are converted into meaningful responses.
//!
//! Rather than being returned to the client as a JSON-RPC response with the `error` field set,
//! they instead indicate a response at the HTTP level only.

use std::fmt::{self, Display, Formatter};

use warp::reject::Reject;

/// Indicates the "Content-Type" header of the request is not "application/json".
///
/// This rejection is converted into an HTTP 415 (unsupported media type) error.
#[derive(Debug)]
pub(crate) struct UnsupportedMediaType;

impl Display for UnsupportedMediaType {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        formatter.write_str("The request's content-type is not supported")
    }
}

impl Reject for UnsupportedMediaType {}

/// Indicates the "Content-Type" header is missing from the request.
///
/// This rejection is converted into an HTTP 400 (bad request) error.
#[derive(Debug)]
pub(crate) struct MissingContentTypeHeader;

impl Display for MissingContentTypeHeader {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        formatter.write_str("The request's content-type is not set")
    }
}

impl Reject for MissingContentTypeHeader {}

/// Indicates the JSON-RPC request is missing the `id` field.
///
/// As per the JSON-RPC specification, this is classed as a Notification and the server should not
/// send a response.  While no JSON-RPC response is generated for this error, we return an HTTP 400
/// (bad request) error, as the node API does not support client Notifications.
#[derive(Debug)]
pub(crate) struct MissingId;

impl Display for MissingId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        formatter.write_str("The request is missing the 'id' field")
    }
}

impl Reject for MissingId {}

/// Indicates the HTTP request body is greater than the maximum allowed.
///
/// Wraps the configured maximum allowed on the server, set via the `max_body_bytes` parameter in
/// `base_filter()`.
#[derive(Debug)]
pub(crate) struct BodyTooLarge(pub(crate) u32);

impl Display for BodyTooLarge {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            formatter,
            "The request payload exceeds the maximum allowed of {} bytes",
            self.0
        )
    }
}

impl Reject for BodyTooLarge {}
