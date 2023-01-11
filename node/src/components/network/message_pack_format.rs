//! Message pack wire format encoder.
//!
//! This module is used to pin the correct version of message pack used throughout the codebase to
//! our network decoder via `Cargo.toml`; using `tokio_serde::MessagePack` would instead tie it
//! to the dependency specified in `tokio_serde`'s `Cargo.toml`.

use std::{
    io::{self, Cursor},
    pin::Pin,
};

use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use tokio_serde::{Deserializer, Serializer};

/// msgpack encoder/decoder for messages.
#[derive(Debug)]
pub struct MessagePackFormat;

impl<M> Serializer<M> for MessagePackFormat
where
    M: Serialize,
{
    // Note: We cast to `io::Error` because of the `Codec::Error: Into<Transport::Error>`
    // requirement.
    type Error = io::Error;

    #[inline]
    fn serialize(self: Pin<&mut Self>, item: &M) -> Result<Bytes, Self::Error> {
        rmp_serde::to_vec(item)
            .map(Into::into)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }
}

impl<M> Deserializer<M> for MessagePackFormat
where
    for<'de> M: Deserialize<'de>,
{
    type Error = io::Error;

    #[inline]
    fn deserialize(self: Pin<&mut Self>, src: &BytesMut) -> Result<M, Self::Error> {
        rmp_serde::from_read(Cursor::new(src))
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }
}
