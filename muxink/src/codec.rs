use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{Sink, SinkExt};
use thiserror::Error;

/// Transcoder.
///
/// A transcoder takes a value of one kind and transforms it to another. Transcoders may contain a
/// state or configuration, which is why this trait is not just a function.
pub trait Transcoder<Input> {
    /// Transcoding error.
    type Error: std::error::Error + Send + Sync + 'static;

    /// The output produced by the transcoder.
    type Output: Send + Sync + 'static;

    /// Transcodes a value.
    ///
    /// When transcoding to type-erased values it should contain the information required for an
    /// accompanying reverse-direction transcode to be able to reconstruct the value from the
    /// transcoded data.
    fn transcode(&mut self, input: Input) -> Result<Self::Output, Self::Error>;
}

/// Error transcoding data for an underlying sink.
#[derive(Debug, Error)]
enum TranscodingSinkError<TransErr, SinkErr> {
    /// The transcoder failed to transcode the given value.
    #[error("transcoding failed")]
    Transcoder(#[source] TransErr),
    /// The wrapped sink returned an error.
    #[error(transparent)]
    Sink(SinkErr),
}

/// A sink adapter for transcoding incoming values into an underlying sink.
struct TranscodingSink<T, Input, S>
where
    T: Transcoder<Input>,
    S: Sink<T::Output>,
{
    /// Transcoder used to transcode data before passing it to the sink.
    transcoder: T,
    /// Underlying sink where data is sent.
    sink: S,
    /// Phantom data to associate the input with this transcoding sink.
    _input_frame: PhantomData<Input>,
}

impl<T, Input, S> Sink<Input> for TranscodingSink<T, Input, S>
where
    Input: Unpin,
    T: Transcoder<Input> + Unpin,
    S: Sink<T::Output> + Unpin,
{
    type Error = TranscodingSinkError<T::Error, S::Error>;

    #[inline]
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();
        self_mut
            .sink
            .poll_ready_unpin(cx)
            .map_err(TranscodingSinkError::Sink)
    }

    #[inline]
    fn start_send(self: Pin<&mut Self>, item: Input) -> Result<(), Self::Error> {
        let self_mut = self.get_mut();

        let transcoded = self_mut
            .transcoder
            .transcode(item)
            .map_err(TranscodingSinkError::Transcoder)?;

        self_mut
            .sink
            .start_send_unpin(transcoded)
            .map_err(TranscodingSinkError::Sink)
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();
        self_mut.poll_flush_unpin(cx)
    }

    #[inline]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();
        self_mut.poll_close_unpin(cx)
    }
}
