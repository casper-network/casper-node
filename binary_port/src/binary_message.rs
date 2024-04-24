#[cfg(test)]
use casper_types::testing::TestRng;
#[cfg(test)]
use rand::Rng;

use bytes::Buf;
use tokio_util::codec::{self};

use crate::error::Error;

type LengthEncoding = u32;
const LENGTH_ENCODING_SIZE_BYTES: usize = std::mem::size_of::<LengthEncoding>();
// TODO[RC]: To config
const MAX_REQUEST_SIZE_BYTES: usize = 1024 * 1024; // 1MB

#[derive(Clone, PartialEq, Debug)]
pub struct BinaryMessage(Vec<u8>);

impl BinaryMessage {
    pub fn new(payload: Vec<u8>) -> Self {
        BinaryMessage(payload)
    }

    pub fn payload(&self) -> &[u8] {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        let len = rng.gen_range(1..=1024);
        let payload = std::iter::repeat_with(|| rng.gen()).take(len).collect();
        BinaryMessage(payload)
    }
}

pub struct BinaryMessageCodec {}

impl codec::Encoder<BinaryMessage> for BinaryMessageCodec {
    type Error = Error;

    fn encode(
        &mut self,
        item: BinaryMessage,
        dst: &mut bytes::BytesMut,
    ) -> Result<(), Self::Error> {
        let length = item.0.len() as LengthEncoding;
        let length_bytes = length.to_le_bytes();
        dst.extend(length_bytes.iter().chain(item.0.iter()));
        Ok(())
    }
}

impl codec::Decoder for BinaryMessageCodec {
    type Item = BinaryMessage;

    type Error = Error;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < LENGTH_ENCODING_SIZE_BYTES {
            // Not enough bytes to read the length.
            return Ok(None);
        }
        let length = LengthEncoding::from_le_bytes([src[0], src[1], src[2], src[3]]) as usize;
        if length > MAX_REQUEST_SIZE_BYTES {
            return Err(Error::RequestTooLarge {
                allowed: MAX_REQUEST_SIZE_BYTES,
                got: length,
            });
        }
        if length == 0 {
            return Err(Error::EmptyRequest);
        }
        if src.len() < length + LENGTH_ENCODING_SIZE_BYTES {
            // Not enough bytes to read the whole message.
            return Ok(None);
        }

        let payload = src[LENGTH_ENCODING_SIZE_BYTES..LENGTH_ENCODING_SIZE_BYTES + length].to_vec();
        src.advance(LENGTH_ENCODING_SIZE_BYTES + length);
        Ok(Some(BinaryMessage(payload)))
    }
}

#[cfg(test)]
mod tests {
    use casper_types::testing::TestRng;
    use tokio_util::codec::{Decoder, Encoder};

    use crate::{
        binary_message::{LengthEncoding, LENGTH_ENCODING_SIZE_BYTES, MAX_REQUEST_SIZE_BYTES},
        error::Error,
        BinaryMessage, BinaryMessageCodec,
    };

    #[test]
    fn binary_message_codec() {
        let rng = &mut TestRng::new();
        let val = BinaryMessage::random(rng);
        let mut codec = BinaryMessageCodec {};
        let mut bytes = bytes::BytesMut::new();
        codec
            .encode(val.clone(), &mut bytes)
            .expect("should encode");

        let decoded = codec
            .decode(&mut bytes)
            .expect("should decode")
            .expect("should be Some");

        assert_eq!(val, decoded);
    }

    #[test]
    fn should_not_decode_when_not_enough_bytes_to_decode_length() {
        let rng = &mut TestRng::new();
        let val = BinaryMessage::random(rng);
        let mut codec = BinaryMessageCodec {};
        let mut bytes = bytes::BytesMut::new();
        codec.encode(val, &mut bytes).expect("should encode");

        let _ = bytes.split_off(LENGTH_ENCODING_SIZE_BYTES / 2);
        let in_bytes = bytes.clone();
        assert!(codec.decode(&mut bytes).expect("should decode").is_none());

        // Ensure that the bytes are not consumed.
        assert_eq!(in_bytes, bytes);
    }

    #[test]
    fn should_not_decode_when_not_enough_bytes_to_decode_full_frame() {
        let rng = &mut TestRng::new();
        let val = BinaryMessage::random(rng);
        let mut codec = BinaryMessageCodec {};
        let mut bytes = bytes::BytesMut::new();
        codec.encode(val, &mut bytes).expect("should encode");

        let _ = bytes.split_off(bytes.len() - 1);
        let in_bytes = bytes.clone();
        assert!(codec.decode(&mut bytes).expect("should decode").is_none());

        // Ensure that the bytes are not consumed.
        assert_eq!(in_bytes, bytes);
    }

    #[test]
    fn should_leave_remainder_in_buffer() {
        let rng = &mut TestRng::new();
        let val = BinaryMessage::random(rng);
        let mut codec = BinaryMessageCodec {};
        let mut bytes = bytes::BytesMut::new();
        codec.encode(val, &mut bytes).expect("should encode");
        let suffix = bytes::Bytes::from_static(b"suffix");
        bytes.extend(&suffix);

        let _ = codec.decode(&mut bytes);

        // Ensure that the bytes are not consumed.
        assert_eq!(bytes, suffix);
    }

    #[test]
    fn should_bail_on_too_large_request() {
        let mut codec = BinaryMessageCodec {};
        let mut bytes = bytes::BytesMut::new();
        let too_large = (MAX_REQUEST_SIZE_BYTES + 1) as LengthEncoding;
        bytes.extend(&too_large.to_le_bytes());

        let result = codec.decode(&mut bytes).unwrap_err();
        assert!(matches!(result, Error::RequestTooLarge { allowed, got }
                 if allowed == MAX_REQUEST_SIZE_BYTES && got == too_large as usize));
    }

    #[test]
    fn should_bail_on_empty_request() {
        let mut codec = BinaryMessageCodec {};
        let mut bytes = bytes::BytesMut::new();
        let empty = 0 as LengthEncoding;
        bytes.extend(&empty.to_le_bytes());

        let result = codec.decode(&mut bytes).unwrap_err();
        assert!(matches!(result, Error::EmptyRequest));
    }
}
