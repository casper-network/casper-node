use std::{pin::Pin, sync::Arc};

use bytes::{Bytes, BytesMut};
use pin_project::pin_project;
use tokio_serde::{Deserializer, Serializer};

use crate::components::networking_metrics::NetworkingMetrics;

use super::{Message, Payload};

#[pin_project]
#[derive(Debug)]
pub(super) struct CountingFormat<F> {
    #[pin]
    inner: F,
    metrics: Arc<NetworkingMetrics>,
}

impl<F> CountingFormat<F> {
    pub(super) fn new(metrics: Arc<NetworkingMetrics>, inner: F) -> Self {
        Self { metrics, inner }
    }
}

impl<F, P> Serializer<Message<P>> for CountingFormat<F>
where
    F: Serializer<Message<P>>,
    P: Payload,
{
    type Error = F::Error;

    #[inline]
    fn serialize(self: Pin<&mut Self>, item: &Message<P>) -> Result<Bytes, Self::Error> {
        let this = self.project();
        let projection: Pin<&mut F> = this.inner;

        let serialized = F::serialize(projection, item)?;
        this.metrics
            .record_payload_out(item.classify(), serialized.len() as u64);

        Ok(serialized)
    }
}

impl<F, P> Deserializer<Message<P>> for CountingFormat<F>
where
    F: Deserializer<Message<P>>,
{
    type Error = F::Error;

    #[inline]
    fn deserialize(self: Pin<&mut Self>, src: &BytesMut) -> Result<Message<P>, Self::Error> {
        let this = self.project();
        let projection: Pin<&mut F> = this.inner;
        F::deserialize(projection, src)
    }
}
