use std::{
    error::Error,
    io,
    marker::PhantomData,
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

    fn send_frame(&mut self, frame: F) -> Self::SendFrameFut;
}

struct Framer<W, F> {
    writer: W,
    _frame_phantom: PhantomData<F>,
}

type FramerFrame<F> = bytes::buf::Chain<u16, F>;

impl<W, F> FrameSink<F> for Framer<W, F>
where
    W: AsyncWrite,
    F: Buf,
{
    type SendFrameFut = GenericBufSender<FramerFrame<F>, W>;

    fn send_frame(&mut self, frame: F) -> Self::SendFrameFut {
        let length_prefixed = ();
        todo!()
    }
}

#[pin_project]
struct GenericBufSender<'a, B, W> {
    buf: B,
    #[pin]
    out: &'a mut W,
}

impl<'a, B, W> Future for GenericBufSender<'a, B, W>
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

struct FramerSendFrame;

impl Future for FramerSendFrame {
    type Output = Result<(), FrameSinkError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        todo!()
    }
}

fn main() {}
