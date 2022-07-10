//! Value or frame transcoding.
//!
//! All operations on values or frame that can be expressed as a one-to-one mapping are performed a
//! using transcoder that implementing the [`Transcoder`] trait.
//!
//! To use transcoders with [`Sink`]s or [`Stream`]s, the [`TranscodingSink`] and
//! [`TranscodingStream`] should be used. Additionally,
//! [`SinkMuxExt::with_transcoder`](crate::SinkMuxExt::with_transcoder) and
//! [`StreamMuxExt::with_transcoder`] provide convenient methods to construct these.
//!
//! # Transcoders and frame decoders
//!
//! A concrete [`Transcoder`] specifies how to translate an input value into an output value. In
//! constrast, a [`FrameDecoder`] is a special decoder that works on a continous stream of bytes (as
//! opposed to already disjunct frames) with the help of an
//! [`io::FrameReader`](crate::io::FrameReader).
//!
//! # Available implementations
//!
//! Currently, the following transcoders and frame decoders are available:
//!
//! * [`length_delimited`]: Transforms byte-like values into self-contained frames with a
//!   length-prefix.

#[cfg(feature = "bincode")]
pub mod bincode;
pub mod length_delimited;

use std::{
    fmt::Debug,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::BytesMut;
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
    /// Note: When transcoding to type-erased values it should contain the information required for
    ///       an accompanying reverse-direction transcode to be able to reconstruct the value from
    ///       the transcoded data.
    fn transcode(&mut self, input: Input) -> Result<Self::Output, Self::Error>;
}

/// Frame decoder.
///
/// A frame decoder extracts a frame from a continous bytestream.
///
/// Note that there is no `FrameEncoder` trait, since the direction would be covered by a "normal"
/// transcoder implementing [`Transcoder`].
pub trait FrameDecoder {
    /// Decoding error.
    type Error: std::error::Error + Send + Sync + 'static;

    type Output: Send + Sync + 'static;

    /// Decodes a frame from a buffer.
    ///
    /// Produces either a frame, an error or an indicator for incompletion. See [`DecodeResult`] for
    /// details.
    ///
    /// Implementers of this function are expected to remove completed frames from `buffer`.
    fn decode_frame(&mut self, buffer: &mut BytesMut) -> DecodeResult<Self::Output, Self::Error>;
}

/// The outcome of a [`decode_frame`] call.
#[derive(Debug, Error)]
pub enum DecodeResult<T, E> {
    /// A complete item was decoded.
    Item(T),
    /// No frame could be decoded, an unknown amount of bytes is still required.
    Incomplete,
    /// No frame could be decoded, but the remaining amount of bytes required is known.
    Remaining(usize),
    /// Irrecoverably failed to decode frame.
    Failed(E),
}

/// Error transcoding data from/for an underlying input/output type.
#[derive(Debug, Error)]
pub enum TranscodingIoError<TransErr, IoErr> {
    /// The transcoder failed to transcode the given value.
    #[error("transcoding failed")]
    Transcoder(#[source] TransErr),
    /// The wrapped input/output returned an error.
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
