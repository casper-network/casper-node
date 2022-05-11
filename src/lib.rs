mod length_prefixed;

use std::{
    error::Error,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Buf;
use futures::{AsyncWrite, Future};
use pin_project::pin_project;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FrameSinkError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Other(Box<dyn Error + Send + Sync>),
}

pub trait FrameSink<F> {
    type SendFrameFut: Future<Output = Result<(), FrameSinkError>> + Send;

    fn send_frame(self, frame: F) -> Self::SendFrameFut;
}

#[pin_project] // TODO: We only need `pin_project` for deriving the `DerefMut` impl we need.
pub struct GenericBufSender<'a, B, W> {
    buf: B,
    out: &'a mut W,
}

impl<'a, B, W> GenericBufSender<'a, B, W> {
    fn new(buf: B, out: &'a mut W) -> Self {
        Self { buf, out }
    }
}

impl<'a, B, W> Future for GenericBufSender<'a, B, W>
where
    B: Buf,
    W: AsyncWrite + Unpin,
{
    type Output = Result<(), FrameSinkError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mref = self.get_mut();
        loop {
            let GenericBufSender {
                ref mut buf,
                ref mut out,
            } = mref;

            let current_slice = buf.chunk();
            let out_pinned = Pin::new(out);

            match out_pinned.poll_write(cx, current_slice) {
                Poll::Ready(Ok(bytes_written)) => {
                    // Record the number of bytes written.
                    buf.advance(bytes_written);
                    if !buf.has_remaining() {
                        // All bytes written, return success.
                        return Poll::Ready(Ok(()));
                    }
                    // We have more data to write, and `out` has not stalled yet, try to send more.
                }
                // An error occured writing, we can just return it.
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error.into())),
                // No writing possible, simply return pending.
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
