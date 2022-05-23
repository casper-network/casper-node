use std::convert::Infallible;

use thiserror::Error;

// TODO: It is probably better to nest error instead, to see clearer what is going on.

/// A frame prefix conversion error.
#[derive(Debug, Error)]
pub enum Error<E = Infallible>
where
    E: std::error::Error,
{
    /// The frame's length cannot be represented with the  prefix.
    #[error("frame too long {actual}/{max}")]
    FrameTooLong { actual: usize, max: usize },
    /// An ACK was received for an item that had not been sent yet.
    #[error("received ACK {actual}, but only sent items up to {expected}")]
    UnexpectedAck { actual: u64, expected: u64 },
    /// Received an ACK for an item that an ACK was already received for.
    #[error("duplicate ACK {actual}, was expecting {expected}")]
    DuplicateAck { actual: u64, expected: u64 },
    /// The ACK stream associated with a backpressured channel was close.d
    #[error("ACK stream closed")]
    AckStreamClosed,
    #[error("ACK stream error")]
    AckStreamError, // TODO: Capture actual ack stream error here.
    /// The wrapped sink returned an error.
    #[error(transparent)]
    Sink(#[from] E),
}
