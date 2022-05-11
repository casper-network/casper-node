use std::marker::PhantomData;

use bytes::Buf;
use futures::AsyncWrite;

use crate::{FrameSink, GenericBufSender, ImmediateFrame};

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

impl<'a, W, F> FrameSink<F> for &'a mut LengthPrefixer<W, F>
where
    W: AsyncWrite + Send + Unpin,
    F: Buf + Send,
{
    type SendFrameFut = GenericBufSender<'a, LengthPrefixedFrame<F>, W>;

    fn send_frame(self, frame: F) -> Self::SendFrameFut {
        let length = frame.remaining() as u16; // TODO: Try into + handle error.
        GenericBufSender::new(ImmediateFrame::from(length).chain(frame), &mut self.writer)
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
