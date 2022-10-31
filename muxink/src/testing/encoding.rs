//! Quickly encoding values.
//!
//! Implements a small encoding scheme for values into raw bytes:
//!
//! * Integers are encoded as little-endian bytestrings.
//! * Single bytes are passed through unchanged.
//! * Chars are encoded as UTF-8 characters.
//!
//! Note that there is no decoding format, as the format is insufficiently framed to allow for easy
//! deserialization.

use std::ops::Deref;

use bytes::Bytes;
use futures::{Sink, SinkExt};

/// A value that is encodable using the testing encoding.
pub(crate) trait TestEncodeable {
    /// Encodes the value to bytes.
    ///
    /// This function is not terribly efficient, but in test code, it does not have to be.
    fn encode(&self) -> Bytes;

    /// Decodes a previously encoded value from bytes.
    ///
    /// The given `raw` buffer must contain exactly the output of a previous `encode` call.
    fn decode(raw: &Bytes) -> Self;
}

impl TestEncodeable for char {
    #[inline]
    fn encode(&self) -> Bytes {
        let mut buf = [0u8; 6];
        let s = self.encode_utf8(&mut buf);
        Bytes::from(s.to_string())
    }

    fn decode(raw: &Bytes) -> Self {
        let s = std::str::from_utf8(&raw).expect("invalid utf8");
        let mut chars = s.chars();
        let c = chars.next().expect("no chars in string");
        assert!(chars.next().is_none());
        c
    }
}

impl TestEncodeable for u8 {
    #[inline]
    fn encode(&self) -> Bytes {
        let raw: Box<[u8]> = Box::new([*self]);
        Bytes::from(raw)
    }

    fn decode(raw: &Bytes) -> Self {
        assert_eq!(raw.len(), 1);
        raw[0]
    }
}

impl TestEncodeable for u16 {
    #[inline]
    fn encode(&self) -> Bytes {
        let raw: Box<[u8]> = Box::new(self.to_le_bytes());
        Bytes::from(raw)
    }

    fn decode(raw: &Bytes) -> Self {
        u16::from_le_bytes(raw.deref().try_into().unwrap())
    }
}

impl TestEncodeable for u32 {
    #[inline]
    fn encode(&self) -> Bytes {
        let raw: Box<[u8]> = Box::new(self.to_le_bytes());
        Bytes::from(raw)
    }

    fn decode(raw: &Bytes) -> Self {
        u32::from_le_bytes(raw.deref().try_into().unwrap())
    }
}

/// Helper trait for quickly encoding and sending a value.
pub(crate) trait EncodeAndSend {
    /// Encode a value using test encoding and send it.
    ///
    /// This is equivalent to the following code:
    ///
    /// ```ignore
    /// let sink: Sink<Bytes> = // ...;
    /// let encoded = value.encode();
    /// sink.send(encoded)
    /// ```
    fn encode_and_send<'a, T>(&'a mut self, value: T) -> futures::sink::Send<'a, Self, Bytes>
    where
        T: TestEncodeable;
}

impl<S> EncodeAndSend for S
where
    S: Sink<Bytes> + Unpin,
{
    fn encode_and_send<'a, T>(&'a mut self, value: T) -> futures::sink::Send<'a, Self, Bytes>
    where
        T: TestEncodeable,
    {
        {
            self.send(value.encode())
        }
    }
}
