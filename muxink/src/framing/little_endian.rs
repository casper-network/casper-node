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
