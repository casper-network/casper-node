//! Length-prefixed frame reading
//!
//! A reader that decodes an incoming stream of length delimited frames into separate frames. Each
//! frame is expected to be prefixed with two bytes representing its length.

use std::{pin::Pin, task::Poll};

use bytes::{Buf, Bytes, BytesMut};
use futures::{AsyncRead, Stream};

use crate::error::Error;

/// Lenght of the prefix that describes the length of the following frame.
const LENGTH_MARKER_SIZE: usize = std::mem::size_of::<u16>();

/// Frame reader for length prefixed frames.
pub struct FrameReader<R: AsyncRead> {
    /// The underlying async bytestream being read.
    stream: R,
    /// Internal buffer for incomplete frames.
    buffer: BytesMut,
    /// Maximum size of a single read call.
    buffer_increment: u16,
}

impl<R: AsyncRead> FrameReader<R> {
    /// Creates a new frame reader on a given stream with the given read buffer increment.
    pub fn new(stream: R, buffer_increment: u16) -> Self {
        Self {
            stream,
            buffer: BytesMut::new(),
            buffer_increment,
        }
    }
}

/// Extracts a length delimited frame from a given buffer.
///
/// If a frame is found, it is split off from the buffer and returned.
fn length_delimited_frame(buffer: &mut BytesMut) -> Result<Option<BytesMut>, Error> {
    let bytes_in_buffer = buffer.remaining();
    if bytes_in_buffer < LENGTH_MARKER_SIZE {
        return Ok(None);
    }
    let data_length = u16::from_le_bytes(
        buffer[0..LENGTH_MARKER_SIZE]
            .try_into()
            .expect("any two bytes should be parseable to u16"),
    ) as usize;

    let end = LENGTH_MARKER_SIZE + data_length;

    if bytes_in_buffer < end {
        return Ok(None);
    }

    let mut full_frame = buffer.split_to(end);
    let _ = full_frame.get_u16_le();

    Ok(Some(full_frame))
}

impl<R> Stream for FrameReader<R>
where
    R: AsyncRead + Unpin,
{
    // TODO: Ultimately, this should become Result<Bytes>.
    type Item = Bytes;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let FrameReader {
            ref mut stream,
            ref mut buffer,
            buffer_increment,
        } = self.get_mut();
        loop {
            match length_delimited_frame(buffer) {
                Ok(Some(frame)) => return Poll::Ready(Some(frame.freeze())),
                Ok(None) => {
                    let start = buffer.len();
                    let end = start + *buffer_increment as usize;
                    buffer.resize(end, 0x00);

                    match Pin::new(&mut *stream).poll_read(cx, &mut buffer[start..end]) {
                        Poll::Ready(Ok(bytes_read)) => {
                            buffer.truncate(start + bytes_read);
                            if bytes_read == 0 {
                                return Poll::Ready(None);
                            }
                        }
                        Poll::Ready(Err(err)) => panic!("poll_read() failed: {}", err),
                        Poll::Pending => return Poll::Pending,
                    }
                }

                Err(err) => panic!("length_delimited_frame() failed: {}", err),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::{Buf, BufMut, BytesMut};
    use futures::{FutureExt, StreamExt};

    use crate::frame_reader::FrameReader;

    use super::length_delimited_frame;

    // In tests use small value to make sure that we correctly merge data that was polled from the
    // stream in small chunks.
    const TESTING_BUFFER_INCREMENT: u16 = 4;

    #[test]
    fn produces_fragments_from_stream() {
        let stream = &b"\x06\x00\x00ABCDE\x06\x00\x00FGHIJ\x03\x00\xffKL\x02\x00\xffM"[..];
        let expected = vec![
            b"\x00ABCDE".to_vec(),
            b"\x00FGHIJ".to_vec(),
            b"\xffKL".to_vec(),
            b"\xffM".to_vec(),
        ];

        let defragmentizer = FrameReader::new(stream, TESTING_BUFFER_INCREMENT);

        let messages: Vec<_> = defragmentizer.collect().now_or_never().unwrap();
        assert_eq!(expected, messages);
    }

    #[test]
    fn extracts_length_delimited_frame() {
        let mut stream = BytesMut::from(&b"\x05\x00ABCDE\x05\x00FGHIJ\x02\x00KL\x01\x00M"[..]);
        let frame = length_delimited_frame(&mut stream).unwrap().unwrap();

        assert_eq!(frame, "ABCDE");
        assert_eq!(stream, b"\x05\x00FGHIJ\x02\x00KL\x01\x00M"[..]);
    }

    #[test]
    fn extracts_length_delimited_frame_single_frame() {
        let mut stream = BytesMut::from(&b"\x01\x00X"[..]);
        let frame = length_delimited_frame(&mut stream).unwrap().unwrap();

        assert_eq!(frame, "X");
        assert!(stream.is_empty());
    }

    #[test]
    fn extracts_length_delimited_frame_empty_buffer() {
        let mut stream = BytesMut::from(&b""[..]);
        let frame = length_delimited_frame(&mut stream).unwrap();

        assert!(frame.is_none());
        assert!(stream.is_empty());
    }

    #[test]
    fn extracts_length_delimited_frame_incomplete_length_in_buffer() {
        let mut stream = BytesMut::from(&b"A"[..]);
        let frame = length_delimited_frame(&mut stream).unwrap();

        assert!(frame.is_none());
        assert_eq!(stream, b"A"[..]);
    }

    #[test]
    fn extracts_length_delimited_frame_incomplete_data_in_buffer() {
        let mut stream = BytesMut::from(&b"\xff\xffABCD"[..]);
        let frame = length_delimited_frame(&mut stream).unwrap();

        assert!(frame.is_none());
        assert_eq!(stream, b"\xff\xffABCD"[..]);
    }

    #[test]
    fn extracts_length_delimited_frame_only_length_in_buffer() {
        let mut stream = BytesMut::from(&b"\xff\xff"[..]);
        let frame = length_delimited_frame(&mut stream).unwrap();

        assert!(frame.is_none());
        assert_eq!(stream, b"\xff\xff"[..]);
    }

    #[test]
    fn extracts_length_delimited_frame_max_size() {
        let mut stream = BytesMut::from(&b"\xff\xff"[..]);
        for _ in 0..u16::MAX {
            stream.put_u8(50);
        }
        let mut frame = length_delimited_frame(&mut stream).unwrap().unwrap();

        assert_eq!(frame.remaining(), u16::MAX as usize);
        for _ in 0..u16::MAX {
            let byte = frame.get_u8();
            assert_eq!(byte, 50);
        }

        assert!(stream.is_empty());
    }
}
