use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Buf;
use futures::{AsyncWrite, Future};
use pin_project::pin_project;

use crate::{FrameSink, FrameSinkError, ImmediateFrame};

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

type LengthPrefixedFrame<F> = bytes::buf::Chain<ImmediateFrame<[u8; 2]>, F>;

impl<W, F> FrameSink<F> for LengthPrefixer<W, F>
where
    W: AsyncWrite + Send + Unpin,
    F: Buf + Send,
{
    // TODO: Remove the `LengthPrefixedFrame` wrapper, make it built into the sender.
    type SendFrameFut = LengthPrefixedFrameSender<W, F>;

    fn send_frame(mut self, frame: F) -> Self::SendFrameFut {
        let length = frame.remaining() as u16; // TODO: Try into + handle error.
        LengthPrefixedFrameSender::new(ImmediateFrame::from(length).chain(frame), self.writer)
    }
}

#[pin_project] // TODO: We only need `pin_project` for deriving the `DerefMut` impl we need.
pub struct LengthPrefixedFrameSender<W, F> {
    buf: LengthPrefixedFrame<F>,
    out: Option<W>,
}

impl<W, F> LengthPrefixedFrameSender<W, F> {
    fn new(buf: LengthPrefixedFrame<F>, out: W) -> Self {
        Self {
            buf,
            out: Some(out),
        }
    }
}

impl<W, F> Future for LengthPrefixedFrameSender<W, F>
where
    F: Buf,
    W: AsyncWrite + Unpin,
{
    type Output = Result<LengthPrefixer<W, F>, FrameSinkError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut out = self
            .out
            .take()
            .expect("(unfused) GenericBufSender polled after completion");

        let mref = self.get_mut();
        let out = loop {
            let LengthPrefixedFrameSender { ref mut buf, .. } = mref;

            let current_slice = buf.chunk();
            let out_pinned = Pin::new(&mut out);

            match out_pinned.poll_write(cx, current_slice) {
                Poll::Ready(Ok(bytes_written)) => {
                    // Record the number of bytes written.
                    buf.advance(bytes_written);
                    if !buf.has_remaining() {
                        // All bytes written, return success.
                        return Poll::Ready(Ok(LengthPrefixer::new(out)));
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

#[cfg(test)]
mod tests {
    use crate::{length_prefixed::LengthPrefixer, FrameSink};

    #[tokio::test]
    async fn length_prefixer_single_frame_works() {
        let mut output = Vec::new();

        let mut lp = LengthPrefixer::new(&mut output);
        let frame = &b"abcdefg"[..];

        assert!(lp.send_frame(frame).await.is_ok());

        assert_eq!(output.as_slice(), b"\x07\x00abcdefg");
    }

    #[tokio::test]
    async fn length_prefixer_multi_frame_works() {
        let mut output = Vec::new();

        let mut lp = LengthPrefixer::new(&mut output);

        assert!(lp.send_frame(&b"one"[..]).await.is_ok());
        assert!(lp.send_frame(&b"two"[..]).await.is_ok());
        assert!(lp.send_frame(&b"three"[..]).await.is_ok());

        assert_eq!(output.as_slice(), b"\x03\x00one\x03\x00two\x05\x00three");
    }
}
