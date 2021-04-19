use std::{
    io,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, Bytes};
use futures_core::{ready, Stream, TryStream};
use futures_sink::Sink;
use pin_project::pin_project;
use serde::{Deserialize, Serialize};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use super::{Message, Transport};

#[pin_project]
pub(super) struct FramedTransport<P> {
    #[pin]
    inner: Framed<Transport, LengthDelimitedCodec>,
    _phantom: PhantomData<P>,
}

impl<P> FramedTransport<P> {
    /// Creates a new `FramedTransport`.
    pub(super) fn new(stream: Transport, maximum_net_message_size: u32) -> Self {
        let inner = Framed::new(
            stream,
            LengthDelimitedCodec::builder()
                .max_frame_length(maximum_net_message_size as usize)
                .new_codec(),
        );

        Self {
            inner,
            _phantom: PhantomData,
        }
    }
}

impl<P> Stream for FramedTransport<P>
where
    for<'a> P: Deserialize<'a>,
{
    type Item = Result<Message<P>, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match ready!(self.as_mut().project().inner.try_poll_next(context)) {
            Some(bytes) => {
                let message: Message<P> = rmp_serde::from_read(bytes?.reader())
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                Poll::Ready(Some(Ok(message)))
            }
            None => Poll::Ready(None),
        }
    }
}

impl<P: Serialize> Sink<Message<P>> for FramedTransport<P> {
    type Error = io::Error;

    fn poll_ready(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_ready(context)
    }

    fn start_send(mut self: Pin<&mut Self>, message: Message<P>) -> Result<(), Self::Error> {
        let bytes = Bytes::from(
            rmp_serde::to_vec(&message)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
        );

        self.as_mut().project().inner.start_send(bytes)?;

        Ok(())
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_flush(context)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().poll_flush(context))?;
        self.project().inner.poll_close(context)
    }
}
