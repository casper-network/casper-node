use bytes::Buf;
use tokio_util::codec;

use crate::error::Error;

type LengthEncoding = u32;
const LENGTH_ENCODING_SIZE_BYTES: usize = std::mem::size_of::<LengthEncoding>();
// TODO[RC]: To config
const MAX_REQUEST_SIZE_BYTES: usize = 1024 * 1024; // 1MB

pub struct BinaryMessage(Vec<u8>);

impl BinaryMessage {
    pub fn new(payload: Vec<u8>) -> Self {
        BinaryMessage(payload)
    }

    pub fn payload(&self) -> &[u8] {
        &self.0
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
