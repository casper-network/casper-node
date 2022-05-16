use bytes::{Buf, Bytes};
use thiserror::Error;

use crate::ImmediateFrame;

pub type SingleChunk = bytes::buf::Chain<ImmediateFrame<[u8; 1]>, Bytes>;

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
    chunk_size: usize,
) -> Result<impl Iterator<Item = SingleChunk>, Error> {
    let frame_size = frame.remaining();
    let num_frames = (frame_size + chunk_size - 1) / chunk_size;

    let chunk_id_ceil: u8 = num_frames.try_into().map_err(|_err| Error::FrameTooLarge {
        chunk_size,
        actual_size: frame_size,
        max_size: u8::MAX as usize * frame_size,
    })?;

    Ok((0..num_frames).into_iter().map(move |n| {
        let chunk_id = if n == 0 {
            chunk_id_ceil
        } else {
            // Will never overflow, since `chunk_id_ceil` already fits into a `u8`.
            n as u8
        };

        let next_chunk_size = chunk_size.min(frame.remaining());
        let chunk_data = frame.copy_to_bytes(next_chunk_size);
        ImmediateFrame::from(chunk_id).chain(chunk_data)
    }))
}

#[cfg(test)]
mod tests {
    use crate::tests::collect_buf;

    use super::chunk_frame;

    #[test]
    fn basic_chunking_works() {
        let frame = b"01234567890abcdefghijklmno";

        let chunks: Vec<_> = chunk_frame(&frame[..], 7)
            .expect("chunking failed")
            .map(collect_buf)
            .collect();

        assert_eq!(
            chunks,
            vec![
                b"\x040123456".to_vec(),
                b"\x017890abc".to_vec(),
                b"\x02defghij".to_vec(),
                b"\x03klmno".to_vec(),
            ]
        );
    }

    #[test]
    fn chunking_with_maximum_size_works() {
        todo!()
    }

    #[test]
    fn chunking_with_too_large_data_fails() {
        todo!()
    }
}
