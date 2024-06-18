#[cfg(test)]
use casper_types::testing::TestRng;
#[cfg(test)]
use rand::Rng;

use bytes::{Buf, Bytes};
use tokio_util::codec::{self};

use crate::error::Error;

type LengthEncoding = u32;
const LENGTH_ENCODING_SIZE_BYTES: usize = std::mem::size_of::<LengthEncoding>();

#[derive(Clone, PartialEq, Debug)]
pub struct BinaryMessage(Bytes);

impl BinaryMessage {
    pub fn new(payload: Vec<u8>) -> Self {
        Self(payload.into())
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

pub struct BinaryMessageCodec {
    max_message_size_bytes: u32,
}

impl BinaryMessageCodec {
    pub fn new(max_message_size_bytes: u32) -> Self {
        Self {
            max_message_size_bytes,
        }
    }

    pub fn max_message_size_bytes(&self) -> u32 {
        self.max_message_size_bytes
    }
}

impl codec::Encoder<BinaryMessage> for BinaryMessageCodec {
    type Error = Error;

    fn encode(
        &mut self,
        item: BinaryMessage,
        dst: &mut bytes::BytesMut,
    ) -> Result<(), Self::Error> {
        let length = item.0.len() as LengthEncoding;
        if length > self.max_message_size_bytes {
            return Err(Error::RequestTooLarge {
                allowed: self.max_message_size_bytes,
                got: length,
            });
        }
        let length_bytes = length.to_le_bytes();
        dst.extend(length_bytes.iter().chain(item.0.iter()));
        Ok(())
    }
}

impl codec::Decoder for BinaryMessageCodec {
    type Item = BinaryMessage;

    type Error = Error;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let (length, have_full_frame) = if let [b1, b2, b3, b4, remainder @ ..] = &src[..] {
            let length = LengthEncoding::from_le_bytes([*b1, *b2, *b3, *b4]) as usize;
            (length, remainder.len() >= length)
        } else {
            // Not enough bytes to read the length.
            return Ok(None);
        };

        if !have_full_frame {
            // Not enough bytes to read the whole message.
            return Ok(None);
        };

        if length > self.max_message_size_bytes as usize {
            return Err(Error::RequestTooLarge {
                allowed: self.max_message_size_bytes,
                got: length as u32,
            });
        }
        if length == 0 {
            return Err(Error::EmptyRequest);
        }

        src.advance(LENGTH_ENCODING_SIZE_BYTES);
        Ok(Some(BinaryMessage(src.split_to(length).freeze())))
    }
}

#[cfg(test)]
mod tests {
    use casper_types::testing::TestRng;
    use rand::Rng;
    use tokio_util::codec::{Decoder, Encoder};

    use crate::{
        binary_message::{LengthEncoding, LENGTH_ENCODING_SIZE_BYTES},
        error::Error,
        BinaryMessage, BinaryMessageCodec,
    };

    const MAX_MESSAGE_SIZE_BYTES: u32 = 1024 * 1024;

    #[test]
    fn binary_message_codec() {
        let rng = &mut TestRng::new();
        let val = BinaryMessage::random(rng);
        let mut codec = BinaryMessageCodec::new(MAX_MESSAGE_SIZE_BYTES);
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
        let mut codec = BinaryMessageCodec::new(MAX_MESSAGE_SIZE_BYTES);
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
        let mut codec = BinaryMessageCodec::new(MAX_MESSAGE_SIZE_BYTES);
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
        let mut codec = BinaryMessageCodec::new(MAX_MESSAGE_SIZE_BYTES);
        let mut bytes = bytes::BytesMut::new();
        codec.encode(val, &mut bytes).expect("should encode");
        let suffix = bytes::Bytes::from_static(b"suffix");
        bytes.extend(&suffix);

        let _ = codec.decode(&mut bytes);

        // Ensure that the bytes are not consumed.
        assert_eq!(bytes, suffix);
    }

    #[test]
    fn encode_should_bail_on_too_large_request() {
        let mut codec = BinaryMessageCodec::new(MAX_MESSAGE_SIZE_BYTES);
        let too_large = MAX_MESSAGE_SIZE_BYTES as usize + 1;
        let val = BinaryMessage::new(vec![0; too_large]);
        let mut bytes = bytes::BytesMut::new();
        let result = codec.encode(val, &mut bytes).unwrap_err();

        assert!(matches!(result, Error::RequestTooLarge { allowed, got }
                 if allowed == codec.max_message_size_bytes && got == too_large as u32));
    }

    #[test]
    fn should_encode_request_of_maximum_size() {
        let mut codec = BinaryMessageCodec::new(MAX_MESSAGE_SIZE_BYTES);
        let just_right_size = MAX_MESSAGE_SIZE_BYTES as usize;
        let val = BinaryMessage::new(vec![0; just_right_size]);
        let mut bytes = bytes::BytesMut::new();

        let result = codec.encode(val, &mut bytes);
        assert!(result.is_ok());
    }

    #[test]
    fn decode_should_bail_on_too_large_request() {
        let rng = &mut TestRng::new();
        let mut codec = BinaryMessageCodec::new(MAX_MESSAGE_SIZE_BYTES);
        let mut bytes = bytes::BytesMut::new();
        let too_large = (codec.max_message_size_bytes + 1) as LengthEncoding;
        bytes.extend(too_large.to_le_bytes());
        bytes.extend(std::iter::repeat_with(|| rng.gen::<u8>()).take(too_large as usize));

        let result = codec.decode(&mut bytes).unwrap_err();
        assert!(matches!(result, Error::RequestTooLarge { allowed, got }
                 if allowed == codec.max_message_size_bytes && got == too_large));
    }

    #[test]
    fn should_decode_request_of_maximum_size() {
        let rng = &mut TestRng::new();
        let mut codec = BinaryMessageCodec::new(MAX_MESSAGE_SIZE_BYTES);
        let mut bytes = bytes::BytesMut::new();
        let just_right_size = (codec.max_message_size_bytes) as LengthEncoding;
        bytes.extend(just_right_size.to_le_bytes());
        bytes.extend(std::iter::repeat_with(|| rng.gen::<u8>()).take(just_right_size as usize));

        let result = codec.decode(&mut bytes);
        assert!(result.is_ok());
    }

    #[test]
    fn should_bail_on_empty_request() {
        let mut codec = BinaryMessageCodec::new(MAX_MESSAGE_SIZE_BYTES);
        let mut bytes = bytes::BytesMut::new();
        let empty = 0 as LengthEncoding;
        bytes.extend(&empty.to_le_bytes());

        let result = codec.decode(&mut bytes).unwrap_err();
        assert!(matches!(result, Error::EmptyRequest));
    }

    #[test]
    fn should_decoded_queued_messages() {
        let rng = &mut TestRng::new();
        let count = rng.gen_range(10000..20000);
        let messages = (0..count)
            .map(|_| BinaryMessage::random(rng))
            .collect::<Vec<_>>();
        let mut codec = BinaryMessageCodec::new(MAX_MESSAGE_SIZE_BYTES);
        let mut bytes = bytes::BytesMut::new();
        for msg in &messages {
            codec
                .encode(msg.clone(), &mut bytes)
                .expect("should encode");
        }

        let mut decoded_messages = vec![];
        loop {
            let maybe_message = codec.decode(&mut bytes).expect("should decode");
            match maybe_message {
                Some(message) => decoded_messages.push(message),
                None => break,
            }
        }

        assert_eq!(messages, decoded_messages);
    }
}
