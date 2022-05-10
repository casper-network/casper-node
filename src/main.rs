use std::{
    error::Error,
    io,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, Bytes};
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

    fn send_frame(&mut self, frame: F) -> Self::SendFrameFut;
}

struct LengthPrefixer<W, F> {
    writer: Option<W>,
    _frame_phantom: PhantomData<F>,
}

// TODO: Instead of bytes, use custom prefixer for small ints, so we do not have to heap allocate.
type LengthPrefixedFrame<F> = bytes::buf::Chain<Bytes, F>;

impl<W, F> FrameSink<F> for LengthPrefixer<W, F>
where
    W: AsyncWrite + Send + Unpin,
    F: Buf + Send,
{
    type SendFrameFut = GenericBufSender<LengthPrefixedFrame<F>, W>;

    fn send_frame(&mut self, frame: F) -> Self::SendFrameFut {
        let length = frame.remaining() as u64; // TODO: Try into + handle error.
        let length_prefixed_frame = Bytes::copy_from_slice(&length.to_le_bytes()).chain(frame);

        let writer = self.writer.take().unwrap(); // TODO: Handle error if missing.

        GenericBufSender::new(length_prefixed_frame, writer)
    }
}

#[pin_project]
struct GenericBufSender<B, W> {
    buf: B,
    #[pin]
    out: W,
}

impl<B, W> GenericBufSender<B, W> {
    fn new(buf: B, out: W) -> Self {
        Self { buf, out }
    }
}

impl<B, W> Future for GenericBufSender<B, W>
where
    B: Buf,
    W: AsyncWrite + Unpin,
{
    type Output = Result<(), FrameSinkError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let current_slice = this.buf.chunk();

        match this.out.poll_write(cx, current_slice) {
            Poll::Ready(Ok(bytes_written)) => {
                // Record the number of bytes written.
                this.buf.advance(bytes_written);
                if this.buf.remaining() == 0 {
                    // All bytes written, return success.
                    Poll::Ready(Ok(()))
                } else {
                    // We have more data to write, come back later.
                    Poll::Pending
                }
            }
            // An error occured writing, we can just return it.
            Poll::Ready(Err(error)) => Poll::Ready(Err(error.into())),
            // No writing possible, simply return pending.
            Poll::Pending => Poll::Pending,
        }
    }
}

fn main() {}
