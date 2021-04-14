use std::pin::Pin;

use bytes::{Bytes, BytesMut};
use pin_project::pin_project;
use tokio_serde::{Deserializer, Serializer};

#[pin_project]
#[derive(Debug)]
pub(super) struct CountingFormat<F> {
    #[pin]
    inner: F,
}

impl<F> CountingFormat<F> {
    pub(super) fn new(inner: F) -> Self {
        Self { inner }
    }
}

impl<F, T> Serializer<T> for CountingFormat<F>
where
    F: Serializer<T>,
{
    type Error = F::Error;

    fn serialize(self: Pin<&mut Self>, item: &T) -> Result<Bytes, Self::Error> {
        let this = self.project();
        let projection: Pin<&mut F> = this.inner;
        F::serialize(projection, item)
    }
}

impl<F, T> Deserializer<T> for CountingFormat<F>
where
    F: Deserializer<T>,
{
    type Error = F::Error;

    #[inline]
    fn deserialize(self: Pin<&mut Self>, src: &BytesMut) -> Result<T, Self::Error> {
        let this = self.project();
        let projection: Pin<&mut F> = this.inner;
        F::deserialize(projection, src)
    }
}
