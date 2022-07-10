pub mod length_delimited;

use std::{
    fmt::Debug,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{ready, Sink, SinkExt, Stream, StreamExt};
use thiserror::Error;

/// Transcoder.
///
/// A transcoder takes a value of one kind and transforms it to another. Transcoders may contain a
/// state or configuration, which is why this trait is not just a function.
pub trait Transcoder<Input> {
    /// Transcoding error.
    type Error: std::error::Error + Debug + Send + Sync + 'static;

    /// The output produced by the transcoder.
    type Output: Send + Sync + 'static;

    /// Transcodes a value.
    ///
    /// When transcoding to type-erased values it should contain the information required for an
    /// accompanying reverse-direction transcode to be able to reconstruct the value from the
    /// transcoded data.
    fn transcode(&mut self, input: Input) -> Result<Self::Output, Self::Error>;
}

/// Error transcoding data from/for an underlying input/output type.
#[derive(Debug, Error)]
pub enum TranscodingIoError<TransErr, IoErr> {
    /// The transcoder failed to transcode the given value.
    #[error("transcoding failed")]
    Transcoder(#[source] TransErr),
    /// The wrapped io returned an error.
    #[error(transparent)]
    Io(IoErr),
}

/// A sink adapter for transcoding incoming values into an underlying sink.
#[derive(Debug)]
pub struct TranscodingSink<T, Input, S>
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

impl<T, Input, S> TranscodingSink<T, Input, S>
where
    T: Transcoder<Input>,
    S: Sink<T::Output>,
{
    /// Creates a new transcoding sink.
    pub fn new(transcoder: T, sink: S) -> Self {
        Self {
            transcoder,
            sink,
            _input_frame: PhantomData,
        }
    }
}

impl<T, Input, S> Sink<Input> for TranscodingSink<T, Input, S>
where
    Input: Unpin + std::fmt::Debug,
    T: Transcoder<Input> + Unpin,
    S: Sink<T::Output> + Unpin,
    T::Output: std::fmt::Debug,
    <S as Sink<T::Output>>::Error: std::error::Error,
{
    type Error = TranscodingIoError<T::Error, S::Error>;

    #[inline]
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();
        self_mut
            .sink
            .poll_ready_unpin(cx)
            .map_err(TranscodingIoError::Io)
    }

    #[inline]
    fn start_send(self: Pin<&mut Self>, item: Input) -> Result<(), Self::Error> {
        let self_mut = self.get_mut();

        let transcoded = self_mut
            .transcoder
            .transcode(item)
            .map_err(TranscodingIoError::Transcoder)?;

        self_mut
            .sink
            .start_send_unpin(transcoded)
            .map_err(TranscodingIoError::Io)
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();
        self_mut
            .sink
            .poll_flush_unpin(cx)
            .map_err(TranscodingIoError::Io)
    }

    #[inline]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();
        self_mut
            .sink
            .poll_close_unpin(cx)
            .map_err(TranscodingIoError::Io)
    }
}

#[derive(Debug)]
pub struct TranscodingStream<T, S> {
    /// Transcoder used to transcode data before returning from the stream.
    transcoder: T,
    /// Underlying stream where data is sent.
    stream: S,
}

impl<T, S> Stream for TranscodingStream<T, S>
where
    T: Transcoder<S::Item> + Unpin,
    S: Stream + Unpin,
{
    type Item = Result<T::Output, T::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let self_mut = self.get_mut();
        match ready!(self_mut.stream.poll_next_unpin(cx)) {
            Some(input) => match self_mut.transcoder.transcode(input) {
                Ok(transcoded) => Poll::Ready(Some(Ok(transcoded))),
                Err(err) => Poll::Ready(Some(Err(err))),
            },
            None => Poll::Ready(None),
        }
    }
}
