/// Length checking pass-through encoder/decoder.
use std::convert::Infallible;

use bytes::{Buf, Bytes, BytesMut};
use thiserror::Error;

/// Fixed-size pass-through encoding/decoding.
use super::{DecodeResult, FrameDecoder, FrameEncoder};

/// Fixed size pass-through encoding/decoding.
///
/// Any frame passed in for encoding is only length checked. Incoming streams are "decoded" by
/// cutting of chunks of the given length.
#[derive(Debug, Default)]
pub struct FixedSize {
    /// The size of frames encoded/decoded.
    size: usize,
}

impl FixedSize {
    /// Creates a new fixed size encoder.
    pub fn new(size: usize) -> Self {
        Self { size }
    }
}

/// An encoding error due to a size mismatch.
#[derive(Copy, Clone, Debug, Error)]
#[error("size of frame at {actual} bytes does not match expected size of {expected} bytes")]
pub struct InvalidSizeError {
    /// The number of bytes expected (configured on the encoder).
    expected: usize,
    /// Actual size passed in.
    actual: usize,
}

impl<T> FrameEncoder<T> for FixedSize
where
    T: Buf + Send,
{
    type Error = InvalidSizeError;
    type Output = T;

    #[inline]
    fn encode_frame(&mut self, buffer: T) -> Result<Self::Output, Self::Error> {
        if buffer.remaining() != self.size {
            Err(InvalidSizeError {
                expected: self.size,
                actual: buffer.remaining(),
            })
        } else {
            Ok(buffer)
        }
    }
}

impl FrameDecoder for FixedSize {
    type Error = Infallible;

    #[inline]
    fn decode_frame(&mut self, buffer: &mut BytesMut) -> DecodeResult<Bytes, Self::Error> {
        if buffer.len() >= self.size {
            DecodeResult::Item(buffer.split_to(self.size).freeze())
        } else {
            DecodeResult::Remaining(self.size - buffer.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use crate::{framing::FrameEncoder, io::FrameReader, testing::collect_stream_results};

    use super::FixedSize;

    /// Decodes the input string, returning the decoded frames and the remainder.
    fn run_decoding_stream(
        input: &[u8],
        size: usize,
        chomp_size: usize,
    ) -> (Vec<Vec<u8>>, Vec<u8>) {
        let mut reader = FrameReader::new(FixedSize::new(size), input, chomp_size);

        let decoded: Vec<_> = collect_stream_results(&mut reader)
            .into_iter()
            .map(|bytes| bytes.into_iter().collect::<Vec<u8>>())
            .collect();

        // Extract the remaining data.
        let (_decoder, remaining_input, buffer) = reader.into_parts();
        let mut remaining = Vec::new();
        remaining.extend(buffer.into_iter());
        remaining.extend(remaining_input);

        (decoded, remaining)
    }

    #[test]
    fn simple_stream_decoding_works() {
        for chomp_size in 1..=1024 {
            let input = b"abcdefghi";
            let (decoded, remainder) = run_decoding_stream(input, 3, chomp_size);
            assert_eq!(decoded, &[b"abc", b"def", b"ghi"]);
            assert!(remainder.is_empty());
        }
    }

    #[test]
    fn stream_decoding_with_remainder_works() {
        for chomp_size in 1..=1024 {
            let input = b"abcdefghijk";
            let (decoded, remainder) = run_decoding_stream(input, 3, chomp_size);
            assert_eq!(decoded, &[b"abc", b"def", b"ghi"]);
            assert_eq!(remainder, b"jk");
        }
    }

    #[test]
    fn empty_stream_is_empty() {
        let input = b"";

        let (decoded, remainder) = run_decoding_stream(input, 3, 5);
        assert!(decoded.is_empty());
        assert!(remainder.is_empty());
    }

    #[test]
    fn encodes_simple_cases_correctly() {
        let seq = &[b"abc", b"def", b"ghi"];

        for &input in seq.into_iter() {
            let mut input = Bytes::from(input.to_vec());
            let mut codec = FixedSize::new(3);

            let outcome = codec
                .encode_frame(&mut input)
                .expect("encoding should not fail")
                .clone();

            assert_eq!(outcome, &input);
        }
    }
}
