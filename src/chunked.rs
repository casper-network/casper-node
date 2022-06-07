//! Chunks frames into pieces.
//!
//! The wire format for chunks is `NCCC...` where `CCC...` is the data chunk and `N` is the
//! continuation byte, which is `0x00` if more chunks are following, `0xFF` if this is the frame's
//! last chunk.

use std::{num::NonZeroUsize, pin::Pin, task::Poll};

use bytes::{Buf, Bytes, BytesMut};
use futures::Stream;

use crate::{error::Error, ImmediateFrame};

pub type SingleChunk = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, Bytes>;

/// Indicator that more chunks are following.
const MORE_CHUNKS: u8 = 0x00;

/// Final chunk indicator.
pub const FINAL_CHUNK: u8 = 0xFF;

pub(crate) struct Defragmentizer<S: Stream> {
    stream: S,
    buffer: Vec<Bytes>,
}

impl<R: Stream> Defragmentizer<R> {
    #[cfg(test)]
    pub(crate) fn new(stream: R) -> Self {
        Self {
            stream,
            buffer: vec![],
        }
    }
}

fn buffer_size_hint(buffer: &mut [Bytes], final_fragment_index: usize) -> usize {
    let maybe_first_fragment = buffer.first();
    match maybe_first_fragment {
        Some(first_fragment) => first_fragment.len() * (final_fragment_index + 1),
        None => 0,
    }
}

fn defragmentize(buffer: &mut Vec<Bytes>) -> Result<Option<BytesMut>, Error> {
    // TODO: We can do better (i.e. without double iteration)
    let last_fragment_index = match buffer
        .iter()
        .enumerate()
        .find(|(_, chunk)| {
            let maybe_first_byte = chunk.first();
            match maybe_first_byte {
                Some(first_byte) => first_byte == &FINAL_CHUNK,
                None => panic!("chunk without continuation byte encountered"),
            }
        })
        .map(|(index, _)| index)
    {
        Some(last_fragment_index) => last_fragment_index,
        None => return Ok(None),
    };

    let mut intermediate_buffer =
        BytesMut::with_capacity(buffer_size_hint(buffer, last_fragment_index));
    buffer
        .iter()
        .take(last_fragment_index + 1)
        .map(|fragment| match fragment.split_first() {
            Some((_, fragment_data)) => fragment_data,
            None => panic!("encountered fragment with zero size"),
        })
        .for_each(|chunk_data| intermediate_buffer.extend(chunk_data));
    buffer.drain(0..last_fragment_index + 1);

    Ok(Some(intermediate_buffer))
}

impl<S> Stream for Defragmentizer<S>
where
    S: Stream + Unpin,
    S: Stream<Item = Bytes>,
{
    type Item = Bytes;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let mut defragmentizer_mut = self.as_mut();
        loop {
            match defragmentize(&mut defragmentizer_mut.buffer) {
                Ok(Some(fragment)) => return Poll::Ready(Some(fragment.freeze())),
                Ok(None) => match Pin::new(&mut defragmentizer_mut.stream).poll_next(cx) {
                    Poll::Ready(Some(chunk)) => defragmentizer_mut.buffer.push(chunk),
                    Poll::Ready(None) => return Poll::Ready(None),
                    Poll::Pending => return Poll::Pending,
                },
                Err(err) => panic!("defragmentize() failed: {}", err),
            }
        }
    }
}

/// Chunks a frame into ready-to-send chunks.
///
/// # Notes
///
/// Internally, data is copied into chunks by using `Buf::copy_to_bytes`. It is advisable to use a
/// `B` that has an efficient implementation for this that avoids copies, like `Bytes` itself.
pub fn chunk_frame<B: Buf>(
    mut frame: B,
    chunk_size: NonZeroUsize,
) -> Result<impl Iterator<Item = SingleChunk>, Error> {
    let chunk_size: usize = chunk_size.into();
    let num_frames = (frame.remaining() + chunk_size - 1) / chunk_size;

    Ok((0..num_frames).into_iter().map(move |_| {
        let remaining = frame.remaining().min(chunk_size);
        let chunk_data = frame.copy_to_bytes(remaining);

        let continuation_byte: u8 = if frame.has_remaining() {
            MORE_CHUNKS
        } else {
            FINAL_CHUNK
        };
        ImmediateFrame::from(continuation_byte).chain(chunk_data)
    }))
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use crate::{
        chunked::{defragmentize, Defragmentizer},
        tests::collect_buf,
    };

    use super::chunk_frame;

    #[test]
    fn basic_chunking_works() {
        let frame = b"01234567890abcdefghijklmno";

        let chunks: Vec<_> = chunk_frame(&frame[..], 7.try_into().unwrap())
            .expect("chunking failed")
            .map(collect_buf)
            .collect();

        assert_eq!(
            chunks,
            vec![
                b"\x000123456".to_vec(),
                b"\x007890abc".to_vec(),
                b"\x00defghij".to_vec(),
                b"\xffklmno".to_vec(),
            ]
        );

        // Try with a chunk size that ends exactly on the frame boundary.
        let frame = b"012345";
        let chunks: Vec<_> = chunk_frame(&frame[..], 3.try_into().unwrap())
            .expect("chunking failed")
            .map(collect_buf)
            .collect();

        assert_eq!(chunks, vec![b"\x00012".to_vec(), b"\xff345".to_vec(),]);
    }

    #[test]
    fn chunking_for_small_size_works() {
        let frame = b"012345";
        let chunks: Vec<_> = chunk_frame(&frame[..], 6.try_into().unwrap())
            .expect("chunking failed")
            .map(collect_buf)
            .collect();

        assert_eq!(chunks, vec![b"\xff012345".to_vec()]);

        // Try also with mismatched chunk size.
        let chunks: Vec<_> = chunk_frame(&frame[..], 15.try_into().unwrap())
            .expect("chunking failed")
            .map(collect_buf)
            .collect();

        assert_eq!(chunks, vec![b"\xff012345".to_vec()]);
    }

    #[test]
    fn defragments() {
        let mut buffer = vec![
            Bytes::from(&b"\x00ABCDE"[..]),
            Bytes::from(&b"\x00FGHIJ"[..]),
            Bytes::from(&b"\xffKL"[..]),
            Bytes::from(&b"\xffM"[..]),
        ];

        let fragment = defragmentize(&mut buffer).unwrap().unwrap();
        assert_eq!(fragment, &b"ABCDEFGHIJKL"[..]);
        let fragment = defragmentize(&mut buffer).unwrap().unwrap();
        assert_eq!(fragment, &b"M"[..]);
    }
}
