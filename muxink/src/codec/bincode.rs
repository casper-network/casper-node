//! Bincode encoding/decoding
//!
//! Both encoding and decoding are supported by this module. Note that `BincodeDecoder`
//! implements both [`Transcoder`] and [`FrameDecoder`]. The former operates on frames and is safe
//! to use, the latter attempts to parse incoming buffers until successful. For this reason,
//! variably sized or large types should be avoided, as decoding will otherwise open up an
//! opportunity for an attacker blow up computational complexity of incoming message parsing.

use std::{
    io::{self, Cursor},
    marker::PhantomData,
};

use bincode::{DefaultOptions, Options};
use bytes::{Buf, Bytes, BytesMut};
use serde::{de::DeserializeOwned, Serialize};

use super::{DecodeResult, FrameDecoder, Transcoder};

/// Bincode encoder.
///
/// Every value is encoded with the default settings of `bincode`.
#[derive(Debug, Default)]
pub struct BincodeEncoder<T> {
    /// Item type processed by this encoder.
    ///
    /// We restrict encoders to a single message type to make decoding on the other end easier.
    item_type: PhantomData<T>,
}

impl<T> BincodeEncoder<T> {
    /// Creates a new bincode encoder.
    pub fn new() -> Self {
        BincodeEncoder {
            item_type: PhantomData,
        }
    }
}

impl<T> Transcoder<T> for BincodeEncoder<T>
where
    T: Serialize,
{
    type Error = bincode::Error;

    type Output = Bytes;

    fn transcode(&mut self, input: T) -> Result<Self::Output, Self::Error> {
        bincode_transcode_options()
            .serialize(&input)
            .map(Bytes::from)
    }
}

/// Bincode decoder.
///
/// Like [`BincodeEncoder`], uses default settings for decoding. Can be used on bytestreams (via
/// [`FrameDecoder`]) as well as frames (through [`Transcoder`]). See module documentation for
/// caveats.
#[derive(Debug, Default)]
pub struct BincodeDecoder<T> {
    item_type: PhantomData<T>,
}

impl<T> BincodeDecoder<T> {
    /// Creates a new bincode decoder.
    pub fn new() -> Self {
        BincodeDecoder {
            item_type: PhantomData,
        }
    }
}

impl<R, T> Transcoder<R> for BincodeDecoder<T>
where
    T: DeserializeOwned + Send + Sync + 'static,
    R: AsRef<[u8]>,
{
    type Error = bincode::Error;

    type Output = T;

    fn transcode(&mut self, input: R) -> Result<Self::Output, Self::Error> {
        bincode_transcode_options().deserialize(input.as_ref())
    }
}

impl<T> FrameDecoder for BincodeDecoder<T>
where
    T: DeserializeOwned + Send + Sync + 'static,
{
    type Error = bincode::Error;
    type Output = T;

    fn decode_frame(&mut self, buffer: &mut BytesMut) -> DecodeResult<Self::Output, Self::Error> {
        let (outcome, consumed) = {
            let slice: &[u8] = buffer.as_ref();
            let mut cursor = Cursor::new(slice);
            let outcome = DefaultOptions::new()
                .with_varint_encoding()
                .allow_trailing_bytes()
                .deserialize_from(&mut cursor);
            (outcome, cursor.position() as usize)
        };

        match outcome {
            Ok(item) => {
                buffer.advance(consumed);
                DecodeResult::Item(item)
            }
            Err(err) => match *err {
                // Note: `bincode::de::read::SliceReader` hardcodes missing data as
                //       `io::ErrorKind::UnexpectedEof`, which is what we match on here. This is a
                //       bit dangerous, since it is not part of the stable API.
                //       TODO: Write test to ensure this is correct.
                bincode::ErrorKind::Io(io_err) if io_err.kind() == io::ErrorKind::UnexpectedEof => {
                    DecodeResult::Incomplete
                }
                bincode::ErrorKind::SizeLimit
                | bincode::ErrorKind::SequenceMustHaveLength
                | bincode::ErrorKind::Custom(_)
                | bincode::ErrorKind::InvalidCharEncoding
                | bincode::ErrorKind::InvalidTagEncoding(_)
                | bincode::ErrorKind::DeserializeAnyNotSupported
                | bincode::ErrorKind::Io(_)
                | bincode::ErrorKind::InvalidUtf8Encoding(_)
                | bincode::ErrorKind::InvalidBoolEncoding(_) => DecodeResult::Failed(err),
            },
        }
    }
}

/// Options for bincode encoding when selecting the bincode format.
pub(crate) fn bincode_transcode_options() -> impl bincode::config::Options {
    DefaultOptions::new()
        .reject_trailing_bytes()
        .with_varint_encoding()
}

#[cfg(test)]
mod tests {
    use super::DecodeResult;
    use crate::codec::{
        bincode::{BincodeDecoder, BincodeEncoder},
        BytesMut, FrameDecoder, Transcoder,
    };

    #[test]
    fn roundtrip() {
        let data = "abc";

        let mut encoder = BincodeEncoder::new();
        let value: String = String::from(data);
        let encoded = encoder.transcode(value).expect("should encode");

        let mut decoder = BincodeDecoder::<String>::new();
        let decoded = decoder.transcode(encoded).expect("should decode");

        assert_eq!(data, decoded);
    }

    #[test]
    fn decodes_frame() {
        let data = b"\x03abc\x04defg";

        let mut bytes: BytesMut = BytesMut::new();
        bytes.extend(data);

        let mut decoder = BincodeDecoder::<String>::new();

        assert!(matches!(decoder.decode_frame(&mut bytes), DecodeResult::Item(i) if i == "abc"));
        assert!(matches!(decoder.decode_frame(&mut bytes), DecodeResult::Item(i) if i == "defg"));
    }

    #[test]
    fn decodes_frame_of_raw_integers() {
        // 40000u16 followed by 7u16
        let data = b"\xfb\x40\x9c\x07";

        let mut bytes: BytesMut = BytesMut::new();
        bytes.extend(data);

        let mut decoder = BincodeDecoder::<u16>::new();

        assert!(matches!(decoder.decode_frame(&mut bytes), DecodeResult::Item(i) if i == 40000));
        assert!(matches!(decoder.decode_frame(&mut bytes), DecodeResult::Item(i) if i == 7));
    }

    #[test]
    fn error_when_decoding_incorrect_data() {
        let data = "abc";

        let mut decoder = BincodeDecoder::<String>::new();
        let _ = decoder.transcode(data).expect_err("should not decode");
    }

    #[test]
    fn error_when_buffer_not_exhausted() {
        let data = b"\x03abc\x04defg";

        let mut decoder = BincodeDecoder::<String>::new();
        let actual_error = *decoder.transcode(data).unwrap_err();

        assert!(
            matches!(actual_error, bincode::ErrorKind::Custom(msg) if msg == "Slice had bytes remaining after deserialization")
        );
    }

    #[test]
    fn error_when_data_incomplete() {
        let data = b"\x03ab";

        let mut bytes: BytesMut = BytesMut::new();
        bytes.extend(data);

        let mut decoder = BincodeDecoder::<String>::new();

        assert!(matches!(
            decoder.decode_frame(&mut bytes),
            DecodeResult::Incomplete
        ));
    }
}
