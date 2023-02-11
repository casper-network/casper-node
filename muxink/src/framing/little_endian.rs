/// Little-endian integer encoding.
use std::{convert::Infallible, marker::PhantomData};

use super::FrameDecoder;

/// Fixed size framing for integers.
///
/// Integers encoded through this codec come out as little endian fixed size bytes; encoding and
/// framing thus happens in a single step. Frame decoding merely splits off an appropriately sized
/// `Bytes` slice, but does not restore the integer from little endian encoding.
#[derive(Debug, Default)]
pub struct LittleEndian<T> {
    /// Phantom data pinning the accepted type.
    ///
    /// While an encoder would not need to restrict `T`, it still is limited to a single type for
    /// type safety.
    _phantom: PhantomData<T>,
}

macro_rules! int_codec {
    ($ty:ty) => {
        impl crate::framing::FrameEncoder<$ty> for LittleEndian<$ty> {
            // Encoding can never fail.
            type Error = Infallible;

            // We use a cursor, which is just a single `usize` of overhead when sending the encoded
            // number.
            type Output = std::io::Cursor<[u8; (<$ty>::BITS / 8) as usize]>;

            fn encode_frame(&mut self, buffer: $ty) -> Result<Self::Output, Self::Error> {
                Ok(std::io::Cursor::new(buffer.to_le_bytes()))
            }
        }

        impl FrameDecoder for LittleEndian<$ty> {
            // Decoding cannot fail, as every bitstring of correct length is a valid integer.
            type Error = Infallible;

            fn decode_frame(
                &mut self,
                buffer: &mut bytes::BytesMut,
            ) -> super::DecodeResult<bytes::Bytes, Self::Error> {
                // Number of bytes to represent the given type.
                const LEN: usize = (<$ty>::BITS / 8) as usize;

                if buffer.len() < LEN {
                    super::DecodeResult::Remaining(LEN - buffer.len())
                } else {
                    let data = buffer.split_to(LEN);
                    super::DecodeResult::Item(data.freeze())
                }
            }
        }
    };
}

// Implement for known integer types.
int_codec!(u16);
int_codec!(u32);
int_codec!(u64);
int_codec!(u128);
int_codec!(i16);
int_codec!(i32);
int_codec!(i64);
int_codec!(i128);

#[cfg(test)]
mod tests {
    use futures::io::Cursor;

    use crate::{framing::FrameEncoder, io::FrameReader, testing::collect_stream_results};

    use super::LittleEndian;

    /// Decodes the input string, returning the decoded frames and the remainder.
    fn run_decoding_stream(input: &[u8], chomp_size: usize) -> (Vec<Vec<u8>>, Vec<u8>) {
        let stream = Cursor::new(input);

        let mut reader = FrameReader::new(LittleEndian::<u32>::default(), stream, chomp_size);

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
    fn simple_stream_decoding_works() {
        for chomp_size in 1..=1024 {
            let input = b"\x01\x02\x03\x04\xAA\xBB\xCC\xDD";
            let (decoded, remainder) = run_decoding_stream(input, chomp_size);
            assert_eq!(decoded, &[b"\x01\x02\x03\x04", b"\xAA\xBB\xCC\xDD"]);
            assert!(remainder.is_empty());
        }
    }

    #[test]
    fn empty_stream_is_empty() {
        let input = b"";

        let (decoded, remainder) = run_decoding_stream(input, 3);
        assert!(decoded.is_empty());
        assert!(remainder.is_empty());
    }

    #[test]
    fn encodes_simple_cases_correctly() {
        let seq = [0x01020304u32, 0xAABBCCDD];
        let outcomes: &[&[u8]] = &[b"\x04\x03\x02\x01", b"\xDD\xCC\xBB\xAA"];

        for (input, expected) in seq.into_iter().zip(outcomes.into_iter()) {
            let mut codec = LittleEndian::<u32>::default();
            let outcome = codec.encode_frame(input).expect("encoding should not fail");
            assert_eq!(outcome.get_ref(), *expected);
        }
    }
}
