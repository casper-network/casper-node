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

    fn send_frame(self, frame: F) -> Self::SendFrameFut;
}

#[derive(Debug)]
pub struct LengthPrefixer<W, F> {
    writer: W,
    _frame_phantom: PhantomData<F>,
}

impl<W, F> LengthPrefixer<W, F> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            _frame_phantom: PhantomData,
        }
    }
}

// TODO: Instead of bytes, use custom prefixer for small ints, so we do not have to heap allocate.
type LengthPrefixedFrame<F> = bytes::buf::Chain<Bytes, F>;

impl<'a, W, F> FrameSink<F> for &'a mut LengthPrefixer<W, F>
where
    W: AsyncWrite + Send + Unpin,
    F: Buf + Send,
{
    type SendFrameFut = GenericBufSender<'a, LengthPrefixedFrame<F>, W>;

    fn send_frame(self, frame: F) -> Self::SendFrameFut {
        let length = frame.remaining() as u64; // TODO: Try into + handle error.
        let length_prefixed_frame = Bytes::copy_from_slice(&length.to_le_bytes()).chain(frame);
        GenericBufSender::new(length_prefixed_frame, &mut self.writer)
    }
}

#[pin_project]
pub struct GenericBufSender<'a, B, W> {
    buf: B,
    #[pin]
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

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let GenericBufSender {
                ref mut buf,
                ref mut out,
            } = &mut *self;

            let current_slice = buf.chunk();
            let out_pinned = Pin::new(out);

            match out_pinned.poll_write(cx, current_slice) {
                Poll::Ready(Ok(bytes_written)) => {
                    // Record the number of bytes written.
                    self.buf.advance(bytes_written);
                    if !self.buf.has_remaining() {
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

#[cfg(test)]
mod tests {
    use crate::{FrameSink, LengthPrefixer};

    #[tokio::test]
    async fn length_prefixer_single_frame_works() {
        let mut output = Vec::new();

        let mut lp = LengthPrefixer::new(&mut output);
        let frame = &b"abcdefg"[..];

        assert!(lp.send_frame(frame).await.is_ok());

        assert_eq!(
            output.as_slice(),
            b"\x07\x00\x00\x00\x00\x00\x00\x00abcdefg"
        );
    }

    #[tokio::test]
    async fn length_prefixer_multi_frame_works() {
        let mut output = Vec::new();

        let mut lp = LengthPrefixer::new(&mut output);

        assert!(lp.send_frame(&b"one"[..]).await.is_ok());
        assert!(lp.send_frame(&b"two"[..]).await.is_ok());
        assert!(lp.send_frame(&b"three"[..]).await.is_ok());

        assert_eq!(
            output.as_slice(),
            b"\x03\x00\x00\x00\x00\x00\x00\x00one\x03\x00\x00\x00\x00\x00\x00\x00two\x05\x00\x00\x00\x00\x00\x00\x00three"
        );
    }
}
