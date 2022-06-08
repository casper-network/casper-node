pub mod backpressured;
pub mod chunked;
pub mod error;
pub mod frame_reader;
pub mod length_prefixed;
pub mod mux;

use bytes::Buf;

/// A frame for stack allocated data.
#[derive(Debug)]
pub struct ImmediateFrame<A> {
    /// How much of the frame has been read.
    pos: usize,
    /// The actual value contained.
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

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Read;

    use bytes::{Buf, Bytes};
    use futures::{future, FutureExt, SinkExt, StreamExt};
    use tokio_util::sync::PollSender;

    use crate::{
        chunked::{make_defragmentizer, make_fragmentizer, SingleChunk},
        frame_reader::FrameReader,
        length_prefixed::{frame_add_length_prefix, LengthPrefixedFrame},
    };

    /// Collects everything inside a `Buf` into a `Vec`.
    pub fn collect_buf<B: Buf>(buf: B) -> Vec<u8> {
        let mut vec = Vec::new();
        buf.reader()
            .read_to_end(&mut vec)
            .expect("reading buf should never fail");
        vec
    }

    /// Test an "end-to-end" instance of the assembled pipeline for sending.
    #[test]
    fn chunked_length_prefixed_sink() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<LengthPrefixedFrame<SingleChunk>>(10);
        let poll_sender = PollSender::new(tx);

        let mut chunked_sink = make_fragmentizer(
            poll_sender.with(|frame| future::ready(frame_add_length_prefix(frame))),
        );

        let sample_data = Bytes::from(&b"QRSTUV"[..]);

        chunked_sink
            .send(sample_data)
            .now_or_never()
            .unwrap()
            .expect("send failed");

        drop(chunked_sink);

        let chunks: Vec<_> = std::iter::from_fn(move || rx.blocking_recv())
            .map(collect_buf)
            .collect();

        assert_eq!(
            chunks,
            vec![b"\x06\x00\x00QRSTU".to_vec(), b"\x02\x00\xffV".to_vec()]
        )
    }

    #[test]
    fn stream_to_message() {
        let stream = &b"\x06\x00\x00ABCDE\x06\x00\x00FGHIJ\x03\x00\xffKL"[..];
        let expected = "ABCDEFGHIJKL";

        let defragmentizer = make_defragmentizer(FrameReader::new(stream));

        let messages: Vec<_> = defragmentizer.collect().now_or_never().unwrap();
        assert_eq!(
            expected,
            messages.first().expect("should have at least one message")
        );
    }

    #[test]
    fn stream_to_multiple_messages() {
        let stream = &b"\x06\x00\x00ABCDE\x06\x00\x00FGHIJ\x03\x00\xffKL\x0d\x00\xffSINGLE_CHUNK\x02\x00\x00C\x02\x00\x00R\x02\x00\x00U\x02\x00\x00M\x02\x00\x00B\x02\x00\xffS"[..];
        let expected = vec!["ABCDEFGHIJKL", "SINGLE_CHUNK", "CRUMBS"];

        let defragmentizer = make_defragmentizer(FrameReader::new(stream));

        let messages: Vec<_> = defragmentizer.collect().now_or_never().unwrap();
        assert_eq!(expected, messages);
    }
}
