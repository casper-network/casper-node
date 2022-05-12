mod chunked;
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

pub trait FrameSink<F>: Sized {
    type SendFrameFut: Future<Output = Result<Self, FrameSinkError>> + Send;

    fn send_frame(self, frame: F) -> Self::SendFrameFut;
}

pub struct ImmediateFrame<A> {
    pos: usize,
    value: A,
}

impl<A> ImmediateFrame<A> {
    #[inline]
    pub fn new(value: A) -> Self {
        Self { pos: 0, value }
    }
}

impl From<u8> for ImmediateFrame<[u8; 1]> {
    #[inline]
    fn from(value: u8) -> Self {
        ImmediateFrame::new(value.to_le_bytes())
    }
}

impl From<u16> for ImmediateFrame<[u8; 2]> {
    #[inline]
    fn from(value: u16) -> Self {
        ImmediateFrame::new(value.to_le_bytes())
    }
}

impl From<u32> for ImmediateFrame<[u8; 4]> {
    #[inline]
    fn from(value: u32) -> Self {
        ImmediateFrame::new(value.to_le_bytes())
    }
}

impl<A> Buf for ImmediateFrame<A>
where
    A: AsRef<[u8]>,
{
    fn remaining(&self) -> usize {
        // Does not overflow, as `pos` is  `< .len()`.

        self.value.as_ref().len() - self.pos
    }

    fn chunk(&self) -> &[u8] {
        // Safe access, as `pos` is guaranteed to be `< .len()`.
        &self.value.as_ref()[self.pos..]
    }

    fn advance(&mut self, cnt: usize) {
        // This is the only function modifying `pos`, upholding the invariant of it being smaller
        // than the length of the data we have.
        self.pos = (self.pos + cnt).min(self.value.as_ref().len());
    }
}

#[pin_project] // TODO: We only need `pin_project` for deriving the `DerefMut` impl we need.
pub struct GenericBufSender<B, W> {
    buf: B,
    out: Option<W>,
}

impl<B, W> GenericBufSender<B, W> {
    fn new(buf: B, out: W) -> Self {
        Self {
            buf,
            out: Some(out),
        }
    }
}

impl<B, W> Future for GenericBufSender<B, W>
where
    B: Buf,
    W: AsyncWrite + Unpin,
{
    type Output = Result<W, FrameSinkError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut out = self
            .out
            .take()
            .expect("(unfused) GenericBufSender polled after completion");

        let mref = self.get_mut();
        let out = loop {
            let GenericBufSender { ref mut buf, .. } = mref;

            let current_slice = buf.chunk();
            let out_pinned = Pin::new(&mut out);

            match out_pinned.poll_write(cx, current_slice) {
                Poll::Ready(Ok(bytes_written)) => {
                    // Record the number of bytes written.
                    buf.advance(bytes_written);
                    if !buf.has_remaining() {
                        // All bytes written, return success.
                        return Poll::Ready(Ok(out));
                    }
                    // We have more data to write, and `out` has not stalled yet, try to send more.
                }
                // An error occured writing, we can just return it.
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error.into())),
                // No writing possible, simply return pending.
                Poll::Pending => {
                    break out;
                }
            }
        };

        mref.out = Some(out);
        Poll::Pending
    }
}
