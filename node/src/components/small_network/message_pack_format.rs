//! Message pack wire format encoder.
//!
//! This module is used to pin the correct version of message pack used throughout the codebase to
//! our network decoder via `Cargo.toml`; using `tokio_serde::MessagePack` would instead tie it
//! to the dependency specified in `tokio_serde`'s `Cargo.toml`.
//!
//! The encoder is also specialized to `Message<P>` instead of a generic payload for simplicity.

use std::{
    io::{self, Cursor},
    pin::Pin,
};

use bytes::{Bytes, BytesMut};
use rmp_serde::{self};
use serde::{Deserialize, Serialize};
use tokio_serde::{Deserializer, Serializer};

use super::Message;

/// msgpack encoder/decoder for messages.
#[derive(Debug)]
pub(super) struct MessagePackFormat;

impl<P> Serializer<Message<P>> for MessagePackFormat
where
    Message<P>: Serialize,
{
    // Note: We cast to `io::Error` because of the `Codec::Error: Into<Transport::Error>`
    // requirement.
    type Error = io::Error;

    #[inline]
    fn serialize(self: Pin<&mut Self>, item: &Message<P>) -> Result<Bytes, Self::Error> {
        rmp_serde::to_vec(item)
            .map(Into::into)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }
}

impl<P> Deserializer<Message<P>> for MessagePackFormat
where
    for<'de> Message<P>: Deserialize<'de>,
{
    type Error = io::Error;

    #[inline]
    fn deserialize(self: Pin<&mut Self>, src: &BytesMut) -> Result<Message<P>, Self::Error> {
        rmp_serde::from_read(Cursor::new(src))
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }
}
