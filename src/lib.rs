pub mod chunked;
pub mod error;
pub mod length_prefixed;

use bytes::Buf;

/// A frame for stack allocated data.
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
    use futures::{future, stream, SinkExt};

    use crate::{
        chunked::{chunk_frame, SingleChunk},
        error::Error,
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
    #[tokio::test]
    async fn chunked_length_prefixed_sink() {
        let base_sink: Vec<LengthPrefixedFrame<SingleChunk>> = Vec::new();

        let length_prefixed_sink =
            base_sink.with(|frame| future::ready(frame_add_length_prefix(frame)));

        let mut chunked_sink = length_prefixed_sink.with_flat_map(|frame| {
            let chunk_iter = chunk_frame(frame, 5.try_into().unwrap()).expect("TODO: Handle error");
            stream::iter(chunk_iter.map(Result::<_, Error>::Ok))
        });

        let sample_data = Bytes::from(&b"abcdef"[..]);

        chunked_sink.send(sample_data).await.expect("send failed");

        let chunks: Vec<_> = chunked_sink
            .into_inner()
            .into_inner()
            .into_iter()
            .map(collect_buf)
            .collect();

        assert_eq!(
            chunks,
            vec![b"\x06\x00\x00abcde".to_vec(), b"\x02\x00\xfff".to_vec()]
        )
    }
}
