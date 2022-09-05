//! 2-byte Length delimited frame encoding/decoding.
//!
//! Allows for frames to be at most `u16::MAX` (64 KB) in size. Frames are encoded by prefixing
//! their length in little endian byte order in front of every frame.
//!
//! The module provides an encoder through the [`Transcoder`] implementation, and a [`FrameDecoder`]
//! for reading these length delimited frames back from a stream.

use std::convert::Infallible;

use bytes::{Buf, Bytes, BytesMut};
use thiserror::Error;

use super::{DecodeResult, FrameDecoder, FrameEncoder};
use crate::ImmediateFrame;

/// Lenght of the prefix that describes the length of the following frame.
const LENGTH_MARKER_SIZE: usize = std::mem::size_of::<u16>();

/// Two-byte length delimited frame encoder and frame decoder.
#[derive(Debug)]
pub struct LengthDelimited;

/// The frame type for length prefixed frames.
pub type LengthPrefixedFrame<F> = bytes::buf::Chain<ImmediateFrame<[u8; 2]>, F>;

impl<B> FrameEncoder<B> for LengthDelimited
where
    B: Buf + Send,
{
    type Error = LengthExceededError;

    type Output = LengthPrefixedFrame<B>;

    fn encode_frame(&mut self, buffer: B) -> Result<Self::Output, Self::Error> {
        let remaining = buffer.remaining();
        let length: u16 = remaining
            .try_into()
            .map_err(|_err| LengthExceededError(remaining))?;
        Ok(ImmediateFrame::from(length).chain(buffer))
    }
}

impl FrameDecoder for LengthDelimited {
    type Error = Infallible;

    fn decode_frame(&mut self, buffer: &mut BytesMut) -> DecodeResult<Bytes, Self::Error> {
        let bytes_in_buffer = buffer.remaining();
        if bytes_in_buffer < LENGTH_MARKER_SIZE {
            return DecodeResult::Incomplete;
        }
        let data_length = u16::from_le_bytes(
            buffer[0..LENGTH_MARKER_SIZE]
                .try_into()
                .expect("any two bytes should be parseable to u16"),
        ) as usize;

        let end = LENGTH_MARKER_SIZE + data_length;

        if bytes_in_buffer < end {
            return DecodeResult::Remaining(end - bytes_in_buffer);
        }

        let mut full_frame = buffer.split_to(end);
        let _ = full_frame.get_u16_le();

        DecodeResult::Item(full_frame.freeze())
    }
}

/// A length-based encoding error.
#[derive(Debug, Error)]
#[error("outgoing frame would exceed maximum frame length of 64 KB: {0}")]
pub struct LengthExceededError(usize);

#[cfg(test)]
mod tests {
    use futures::io::Cursor;

    use crate::{
        io::FrameReader,
        testing::{collect_stream_results, TESTING_BUFFER_INCREMENT},
    };

    use super::LengthDelimited;

    /// Decodes the input string, returning the decoded frames and the remainder.
    fn run_decoding_stream(input: &[u8]) -> (Vec<Vec<u8>>, Vec<u8>) {
        let stream = Cursor::new(input);

        let mut reader = FrameReader::new(LengthDelimited, stream, TESTING_BUFFER_INCREMENT);

        let decoded: Vec<_> = collect_stream_results(&mut reader)
            .into_iter()
            .map(|bytes| bytes.into_iter().collect::<Vec<u8>>())
            .collect();

        // Extract the remaining data.
        let (_decoder, cursor, buffer) = reader.into_parts();
        let mut remaining = Vec::new();
        remaining.extend(buffer.into_iter());
        let cursor_pos = cursor.position() as usize;
        remaining.extend(&cursor.into_inner()[cursor_pos..]);

        (decoded, remaining)
    }

    #[test]
    fn produces_fragments_from_stream() {
        let input = &b"\x06\x00\x00ABCDE\x06\x00\x00FGHIJ\x03\x00\xffKL\x02\x00\xffM"[..];
        let expected: &[&[u8]] = &[b"\x00ABCDE", b"\x00FGHIJ", b"\xffKL", b"\xffM"];

        let (decoded, remainder) = run_decoding_stream(input);

        assert_eq!(expected, decoded);
        assert!(remainder.is_empty());
    }

    #[test]
    fn extracts_length_delimited_frame_single_frame() {
        let input = b"\x01\x00X";

        let (decoded, remainder) = run_decoding_stream(input);
        assert_eq!(decoded, &[b"X"]);
        assert!(remainder.is_empty());
    }

    #[test]
    fn extracts_length_delimited_frame_empty_buffer() {
        let input: &[u8] = b"";
        let (decoded, remainder) = run_decoding_stream(input);

        assert!(decoded.is_empty());
        assert!(remainder.is_empty());
    }

    #[test]
    fn extracts_length_delimited_frame_incomplete_length_in_buffer() {
        let input = b"A";

        let (decoded, remainder) = run_decoding_stream(input);

        assert!(decoded.is_empty());
        assert_eq!(remainder, b"A");
    }

    #[test]
    fn extracts_length_delimited_frame_incomplete_data_in_buffer() {
        let input = b"\xff\xffABCD";

        let (decoded, remainder) = run_decoding_stream(input);

        assert!(decoded.is_empty());

        assert_eq!(remainder, b"\xff\xffABCD"[..]);
    }

    #[test]
    fn extracts_length_delimited_frame_only_length_in_buffer() {
        let input = b"\xff\xff";

        let (decoded, remainder) = run_decoding_stream(input);

        assert!(decoded.is_empty());
        assert_eq!(remainder, b"\xff\xff"[..]);
    }

    #[test]
    fn extracts_length_delimited_frame_max_size() {
        let mut input = Vec::from(&b"\xff\xff"[..]);
        input.resize(u16::MAX as usize + 2, 50);
        let (decoded, remainder) = run_decoding_stream(&input);

        assert_eq!(decoded, &[&input[2..]]);
        assert!(remainder.is_empty());
    }
}
