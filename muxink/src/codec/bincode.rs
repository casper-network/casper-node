//! Bincode encoding/decoding
//!
//! Both encoding and decoding are supported by this module. Note that `BincodeDecoder` supports
//! implements both [`Transcoder`] and [`FrameDecoder`]. The former operates on frames and is safe
//! to use, the latter attempts to parse incoming buffers until successful. For this reason,
//! variably sized or large types should be avoided, as decoding will otherwise open up an
//! opportunity for an attacker blow up computational complexity of incoming message parsing.

use std::{
    io::{self, Cursor},
    marker::PhantomData,
};

use bytes::{Buf, Bytes, BytesMut};
use serde::{de::DeserializeOwned, Serialize};

use super::{DecodeResult, FrameDecoder, Transcoder};

/// A bincode encoder.
///
/// Every value is encoded with the default settings of `bincode`.
#[derive(Default)]
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
        bincode::serialize(&input).map(Bytes::from)
    }
}

/// Bincode decoder.
///
/// Like [`BincodeEncoder`], uses default settings for decoding. Can be used on bytestreams (via
/// [`FrameDecoder`]) as well as frames (through [`Transcoder`]). See module documentation for
/// caveats.
#[derive(Default)]
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
        bincode::deserialize(input.as_ref())
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
            let outcome = bincode::deserialize_from(&mut cursor);
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
                _ => DecodeResult::Failed(err),
            },
        }
    }
}
