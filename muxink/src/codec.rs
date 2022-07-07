use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{Sink, SinkExt};
use thiserror::Error;

/// Encoder.
///
/// An encoder takes a value of one kind and transforms it to another. Encoders may contain a state
/// or configuration, which is why this trait is not just a function.
pub trait Encoder<Input> {
    /// Encoding error.
    type Error: std::error::Error + Send + Sync + 'static;

    /// The output produced by the encoder.
    type Output: Send + Sync + 'static;

    /// Encodes a value.
    ///
    /// When encoding to type-erased values it must contain the information required for an
    /// accompanying `Decoder` to be able to reconstruct the value from the encoded data.
    fn encode(&mut self, input: Input) -> Result<Self::Output, Self::Error>;
}

/// Error encoding data for an underlying sink.
#[derive(Debug, Error)]
enum EncodingSinkError<EncErr, SinkErr> {
    /// The encoder failed to encode the given value.
    #[error("encoding failed")]
    Encoder(#[source] EncErr),
    /// The wrapped sink returned an error.
    #[error(transparent)]
    Sink(SinkErr),
}

/// A sink adapter for encoding incoming values into an underlying sink.
struct EncodingSink<E, Input, S>
where
    E: Encoder<Input>,
    S: Sink<E::Output>,
{
    /// Encoder used to encode data before passing it to the sink.
    encoder: E,
    /// Underlying sink where data is sent.
    sink: S,
    /// Phantom data to associate the input with this encoding sink.
    _input_frame: PhantomData<Input>,
}

impl<E, Input, S> Sink<Input> for EncodingSink<E, Input, S>
where
    Input: Unpin,
    E: Encoder<Input> + Unpin,
    S: Sink<E::Output> + Unpin,
{
    type Error = EncodingSinkError<E::Error, S::Error>;

    #[inline]
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let self_mut = self.get_mut();
        self_mut
            .sink
            .poll_ready_unpin(cx)
            .map_err(EncodingSinkError::Sink)
    }

    #[inline]
    fn start_send(self: Pin<&mut Self>, item: Input) -> Result<(), Self::Error> {
        let self_mut = self.get_mut();

        let encoded = self_mut
            .encoder
            .encode(item)
            .map_err(EncodingSinkError::Encoder)?;

        self_mut
            .sink
            .start_send_unpin(encoded)
            .map_err(EncodingSinkError::Sink)
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
