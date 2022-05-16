//! Chunks frames into pieces.
//!
//! The wire format for chunks is `NCCC...` where `CCC...` is the data chunk and `N` is the
//! continuation byte, which is `0x00` if more chunks are following, `0xFF` if this is the frame's
//! last chunk.

use std::num::NonZeroUsize;

use bytes::{Buf, Bytes};
use thiserror::Error;

use crate::ImmediateFrame;

pub type SingleChunk = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, Bytes>;

/// Indicator that more chunks are following.
const MORE_CHUNKS: u8 = 0x00;

/// Final chunk indicator.
const FINAL_CHUNK: u8 = 0xFF;

#[derive(Debug, Error)]
pub enum Error {
    #[error("file of {} be chunked into {chunk_size} byte chunks, exceeds max")]
    FrameTooLarge {
        chunk_size: usize,
        actual_size: usize,
        max_size: usize,
    },
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
    use crate::tests::collect_buf;

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
}
