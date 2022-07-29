//! Bytesrepr encoding/decoding
//!
//! Both encoding and decoding are supported by this module. Note that `BytesreprDecoder`
//! implements both [`Transcoder`] and [`FrameDecoder`].

use bytes::{Buf, Bytes, BytesMut};
use casper_types::bytesrepr::{self, FromBytes, ToBytes};
use std::{fmt::Debug, marker::PhantomData};
use thiserror::Error;

use super::{DecodeResult, FrameDecoder, Transcoder};
use crate::codec::DecodeResult::Failed;

#[derive(Debug, Error)]
#[error("bytesrepr error")]
pub struct Error(bytesrepr::Error);

/// Bytesrepr encoder.
#[derive(Debug, Default)]
pub struct BytesreprEncoder<T> {
    /// Item type processed by this encoder.
    ///
    /// We restrict encoders to a single message type to make decoding on the other end easier.
    item_type: PhantomData<T>,
}

impl<T> BytesreprEncoder<T> {
    /// Creates a new bytesrepr encoder.
    pub fn new() -> Self {
        BytesreprEncoder {
            item_type: PhantomData,
        }
    }
}

impl<T> Transcoder<T> for BytesreprEncoder<T>
where
    T: ToBytes,
{
    type Error = Error;

    type Output = Bytes;

    fn transcode(&mut self, input: T) -> Result<Self::Output, Self::Error> {
        Ok(input.into_bytes().map_err(|err| Error(err))?.into())
    }
}

/// Bytesrepr decoder.
#[derive(Debug, Default)]
pub struct BytesreprDecoder<T> {
    item_type: PhantomData<T>,
}

impl<T> BytesreprDecoder<T> {
    /// Creates a new bytesrepr decoder.
    pub fn new() -> Self {
        BytesreprDecoder {
            item_type: PhantomData,
        }
    }
}

impl<R, T> Transcoder<R> for BytesreprDecoder<T>
where
    T: FromBytes + Send + Sync + 'static,
    R: AsRef<[u8]>,
{
    type Error = Error;

    type Output = T;

    fn transcode(&mut self, input: R) -> Result<Self::Output, Self::Error> {
        Ok(bytesrepr::deserialize_from_slice(input).map_err(|eee| Error(eee))?)
    }
}

impl<T> FrameDecoder for BytesreprDecoder<T>
where
    T: FromBytes + Send + Sync + 'static,
{
    type Error = Error;
    type Output = T;

    fn decode_frame(&mut self, buffer: &mut BytesMut) -> DecodeResult<Self::Output, Self::Error> {
        let transcoded = FromBytes::from_bytes(buffer.as_ref());
        match transcoded {
            Ok((data, rem)) => {
                let _ = buffer.split_to(buffer.remaining() - rem.len());
                DecodeResult::Item(data)
            }
            Err(err) => match &err {
                bytesrepr::Error::EarlyEndOfStream => DecodeResult::Incomplete,
                bytesrepr::Error::Formatting
                | bytesrepr::Error::LeftOverBytes
                | bytesrepr::Error::NotRepresentable
                | bytesrepr::Error::ExceededRecursionDepth
                | bytesrepr::Error::OutOfMemory => Failed(Error(err)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DecodeResult, Error};
    use crate::codec::{
        bytesrepr::{BytesreprDecoder, BytesreprEncoder},
        BytesMut, FrameDecoder, Transcoder,
    };
    use casper_types::bytesrepr;

    #[test]
    fn roundtrip() {
        let data = "abc";

        let mut encoder = BytesreprEncoder::new();
        let value: String = String::from(data);
        let encoded = encoder.transcode(value).expect("should encode");

        let mut decoder = BytesreprDecoder::<String>::new();
        let decoded = decoder.transcode(encoded).expect("should decode");

        assert_eq!(data, decoded);
    }

    #[test]
    fn decodes_frame() {
        let data = b"\x03\0\0\0abc\x04\0\0\0defg";

        let mut bytes: BytesMut = BytesMut::new();
        bytes.extend(data);

        let mut decoder = BytesreprDecoder::<String>::new();

        assert!(matches!(decoder.decode_frame(&mut bytes), DecodeResult::Item(i) if i == "abc"));
        assert!(matches!(decoder.decode_frame(&mut bytes), DecodeResult::Item(i) if i == "defg"));
    }

    #[test]
    fn error_when_decoding_incorrect_data() {
        let data = "abc";

        let mut decoder = BytesreprDecoder::<String>::new();
        let _ = decoder.transcode(data).expect_err("should not decode");
    }

    #[test]
    fn error_when_buffer_not_exhausted() {
        let data = b"\x03\0\0\0abc\x04\0\0\0defg";

        let mut decoder = BytesreprDecoder::<String>::new();
        let actual_error = decoder.transcode(data).unwrap_err();

        assert!(matches!(
            actual_error,
            Error(bytesrepr::Error::LeftOverBytes)
        ));
    }

    #[test]
    fn error_when_data_incomplete() {
        let data = b"\x03\0\0\0ab";

        let mut decoder = BytesreprDecoder::<String>::new();
        let actual_error = decoder.transcode(data).unwrap_err();

        assert!(matches!(
            actual_error,
            Error(bytesrepr::Error::EarlyEndOfStream)
        ));
    }
}
