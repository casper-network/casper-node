//! Error type for `juliet`.

use thiserror::Error;

/// Protocol violation.
#[derive(Debug, Error)]
pub enum Error {
    /// The peer sent invalid flags in a header.
    #[error("invalid flags: {0:010b}")]
    InvalidFlags(u8),
    /// A channel number that does not exist was encountered.
    #[error("invalid channel: {0}")]
    InvalidChannel(u8),
    /// Peer made too many requests (without awaiting sufficient responses).
    #[error("request limit exceeded")]
    RequestLimitExceeded,
    /// Peer re-used an in-flight request ID.
    #[error("duplicate request id")]
    DuplicateRequest,
}
