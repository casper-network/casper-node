//! Chunks frames into pieces.
//!
//! The wire format for chunks is `NCCC...` where `CCC...` is the data chunk and `N` is the
//! continuation byte, which is `0x00` if more chunks are following, `0xFF` if this is the frame's
//! last chunk.

use std::{future, num::NonZeroUsize};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::{Stream, StreamExt};

use crate::{error::Error, ImmediateFrame};

pub type SingleChunk = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, Bytes>;

/// Indicator that more chunks are following.
pub const MORE_CHUNKS: u8 = 0x00;

/// Final chunk indicator.
pub const FINAL_CHUNK: u8 = 0xFF;

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

pub(crate) fn make_defragmentizer<S: Stream<Item = Bytes>>(source: S) -> impl Stream<Item = Bytes> {
    let mut buffer = vec![];
    source.filter_map(move |mut fragment| {
        let first_byte = *fragment.first().expect("missing first byte");
        buffer.push(fragment.split_off(1));
        match first_byte {
            FINAL_CHUNK => {
                // TODO: Check the true zero-copy approach.
                let mut buf = BytesMut::new();
                for fragment in buffer.drain(..) {
                    buf.put_slice(&fragment);
                }
                return future::ready(Some(buf.freeze()));
            }
            MORE_CHUNKS => return future::ready(None),
            _ => panic!("garbage found where continuation byte was expected"),
        }
    })
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

    // #[test]
    // fn defragments() {
    //     let mut buffer = vec![
    //         Bytes::from(&b"\x00ABCDE"[..]),
    //         Bytes::from(&b"\x00FGHIJ"[..]),
    //         Bytes::from(&b"\xffKL"[..]),
    //         Bytes::from(&b"\xffM"[..]),
    //     ];

    //     let fragment = defragmentize(&mut buffer).unwrap().unwrap();
    //     assert_eq!(fragment, &b"ABCDEFGHIJKL"[..]);
    //     let fragment = defragmentize(&mut buffer).unwrap().unwrap();
    //     assert_eq!(fragment, &b"M"[..]);
    // }
}
