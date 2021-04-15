//! Observability for network serialization/deserialization.

use std::{
    fmt::{self, Display, Formatter},
    pin::Pin,
    sync::Arc,
};

use bytes::{Bytes, BytesMut};
use pin_project::pin_project;
use tokio_serde::{Deserializer, Serializer};
use tracing::trace;

use crate::{components::networking_metrics::NetworkingMetrics, crypto::hash};

use super::{Message, Payload};

/// Lazily-evaluated network message ID generator.
///
/// Calculates a hash for the wrapped value when `Display::fmt` is called.
#[derive(Debug)]
struct TraceId<'a>(&'a [u8]);

impl<'a> Display for TraceId<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:x}", hash::hash(self.0))
    }
}

/// A metric-updating serializer/deserializer wrapper for network messages.
///
/// Classifies each message given and updates the `NetworkingMetrics` accordingly. Also emits a
/// TRACE-level message to the `net_out` and `net_in` target with a per-message unique hash when
/// a message is sent or received.
#[pin_project]
#[derive(Debug)]
pub(super) struct CountingFormat<F> {
    /// The actual serializer performing the work.
    #[pin]
    inner: F,
    /// Metrics to update.
    metrics: Arc<NetworkingMetrics>,
}

impl<F> CountingFormat<F> {
    /// Creates a new counting formatter.
    #[inline]
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
        let msg_size = serialized.len() as u64;
        let msg_kind = item.classify();
        this.metrics.record_payload_out(msg_kind, msg_size);

        trace!(target: "net_out",
            msg_id = %TraceId(&serialized),
            msg_size,
            msg_kind = %msg_kind, "sending");

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

        // We do not include additional meta info here, since we do not want the deserialization
        // time to be added to our measurements.
        trace!(target: "net_in",
            msg_id=%TraceId(&src),
            "received");

        F::deserialize(projection, src)
    }
}
