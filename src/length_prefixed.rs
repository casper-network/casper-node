use std::marker::PhantomData;

use bytes::{Buf, Bytes};
use futures::AsyncWrite;

use crate::{FrameSink, GenericBufSender};

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

#[cfg(test)]
mod tests {
    use crate::{length_prefixed::LengthPrefixer, FrameSink};

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
