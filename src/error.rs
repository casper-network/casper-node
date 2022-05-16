use std::convert::Infallible;

use thiserror::Error;

/// A frame prefix conversion error.
#[derive(Debug, Error)]
pub enum Error<E = Infallible>
where
    E: std::error::Error,
{
    /// The frame's length cannot be represented with the  prefix.
    #[error("frame too long {actual}/{max}")]
    FrameTooLong { actual: usize, max: usize },
    #[error(transparent)]
    Sink(#[from] E),
}
