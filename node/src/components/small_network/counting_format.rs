//! Observability for network serialization/deserialization.
//!
//! This module introduces two IDs: [`ConnectionId`] and [`TraceId`]. The [`ConnectionId`] is a
//! unique ID per established connection that can be independently derive by peers on either of a
//! connection. [`TraceId`] identifies a single message, distinguishing even messages that are sent
//! to the same peer with equal contents.

use std::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
    pin::Pin,
    sync::Arc,
};

use bytes::{Bytes, BytesMut};
use hex_fmt::HexFmt;
use openssl::ssl::SslRef;
use pin_project::pin_project;
use static_assertions::const_assert;
use tokio_serde::{Deserializer, Serializer};
use tracing::{error, trace};

use super::{tls::KeyFingerprint, Message, Payload};
#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    components::networking_metrics::NetworkingMetrics,
    crypto::hash::{self, Digest},
    types::NodeId,
    utils,
};

/// Lazily-evaluated network message ID generator.
///
/// Calculates a hash for the wrapped value when `Display::fmt` is called.
#[derive(Debug)]
struct TraceId([u8; 8]);

impl Display for TraceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{:x}", HexFmt(&self.0)))
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
    /// Identifier for the connection.
    connection_id: ConnectionId,
    /// Counter for outgoing messages.
    out_count: u64,
    /// Counter for incoming messages.
    in_count: u64,
    /// Our role in the connection.
    role: Role,
    /// Metrics to update.
    metrics: Arc<NetworkingMetrics>,
}

impl<F> CountingFormat<F> {
    /// Creates a new counting formatter.
    #[inline]
    pub(super) fn new(
        metrics: Arc<NetworkingMetrics>,
        connection_id: ConnectionId,
        role: Role,
        inner: F,
    ) -> Self {
        Self {
            metrics,
            connection_id,
            out_count: 0,
            in_count: 0,
            role,
            inner,
        }
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

        let trace_id = this
            .connection_id
            .create_trace_id(this.role.out_flag(), *this.out_count);
        *this.out_count += 1;

        trace!(target: "net_out",
            msg_id = %trace_id,
            msg_size,
            msg_kind = %msg_kind, "sending");

        Ok(serialized)
    }
}

impl<F, P> Deserializer<Message<P>> for CountingFormat<F>
where
    F: Deserializer<Message<P>>,
    P: Payload,
{
    type Error = F::Error;

    #[inline]
    fn deserialize(self: Pin<&mut Self>, src: &BytesMut) -> Result<Message<P>, Self::Error> {
        let this = self.project();
        let projection: Pin<&mut F> = this.inner;

        let msg_size = src.len() as u64;

        // We do not include additional meta info here, since we do not want the deserialization
        // time to be added to our measurements.
        let deserialized = F::deserialize(projection, src)?;
        let msg_kind = deserialized.classify();

        let trace_id = this
            .connection_id
            .create_trace_id(this.role.in_flag(), *this.in_count);
        *this.in_count += 1;

        trace!(target: "net_in",
            msg_id = %trace_id,
            msg_size,
            msg_kind = %msg_kind, "sending");

        Ok(deserialized)
    }
}

/// An ID identifying a connection.
///
/// The ID is guaranteed to be the same on both ends of the connection, but not guaranteed to be
/// unique or sufficiently random. Do not use it for any cryptographic/security related purposes.
#[derive(Debug)]
pub(super) struct ConnectionId([u8; Digest::LENGTH]);

// Invariant assumed by `ConnectionId`, `Digest` must be <= than `KeyFingerprint`.
const_assert!(KeyFingerprint::LENGTH >= Digest::LENGTH);
// We also assume it is at least 12 bytes.
const_assert!(Digest::LENGTH >= 12);

/// Random data derived from TLS connections.
#[derive(Copy, Clone, Debug)]
pub(super) struct TlsRandomData {
    /// Random data extract from the client of the connection.
    combined_random: [u8; 12],
}

impl TlsRandomData {
    /// Collects random data from an existing SSL collection.
    ///
    /// Ideally we would use the TLS session ID, but it is not available on outgoing connections at
    /// the times we need it. Instead, we use the `server_random` and `client_random` nonces, which
    /// will be the same on both ends of the connection.
    fn collect(ssl: &SslRef) -> Self {
        // We are using only the first 12 bytes of these 32 byte values here, just in case we missed
        // something in our assessment that hashing these should be safe. Additionally, these values
        // are XOR'd, not concatenated. All this is done to prevent leaking information about these
        // numbers.
        //
        // Some SSL implementations use timestamps for the first four bytes, so to be sufficiently
        // random, we use 4 + 8 bytes of the nonces.
        let mut server_random = [0; 12];
        let mut client_random = [0; 12];

        ssl.server_random(&mut server_random);
        ssl.client_random(&mut client_random);

        Self {
            combined_random: server_random,
        }
    }
}

impl ConnectionId {
    /// Creates a new connection ID, based on random values from server and client, as well as
    /// node IDs.
    fn create(random_data: TlsRandomData, our_id: NodeId, their_id: NodeId) -> ConnectionId {
        // Hash the resulting random values.
        let mut id = hash::hash(random_data.combined_random).to_array();

        // We XOR in a hashes of server and client fingerprint, to ensure that in the case of an
        // accidental collision (e.g. when `server_random` and `client_random` turn out to be all
        // zeros), we still have a chance of producing a reasonable ID.
        if let Some(our_id_bytes) = our_id.hash_bytes() {
            utils::xor(&mut id, &our_id_bytes[0..Digest::LENGTH]);
        } else {
            error!(
                ?our_id,
                "small_network attempted to retrieve bytes of ID, but seems to be libp2p ID?"
            )
        }

        if let Some(their_id_bytes) = their_id.hash_bytes() {
            utils::xor(&mut id, &their_id_bytes[0..Digest::LENGTH]);
        } else {
            error!(
                ?their_id,
                "small_network attempted to retrieve bytes of ID, but seems to be libp2p ID?"
            )
        }

        ConnectionId(id)
    }

    /// Creates a new [`TraceID`] based on the message count.
    fn create_trace_id(&self, flag: u8, count: u64) -> TraceId {
        // Copy the basic network ID.
        let mut buffer = self.0;

        // Direction set on first byte.
        buffer[0] ^= flag;

        // XOR in message count.
        utils::xor(&mut buffer[4..12], &count.to_ne_bytes());

        // Hash again and truncate.
        let full_hash = hash::hash(&buffer);

        // Safe to expect here, as we assert earlier that `Digest` is at least 12 bytes.
        let truncated =
            TryFrom::try_from(&full_hash.to_array()[0..8]).expect("buffer size mismatch");

        TraceId(truncated)
    }

    /// Creates a new connection ID from an existing SSL connection.
    #[inline]
    pub(crate) fn from_connection(ssl: &SslRef, our_id: NodeId, their_id: NodeId) -> Self {
        Self::create(TlsRandomData::collect(ssl), our_id, their_id)
    }

    /// Creates a random `ConnectionId`.
    #[cfg(test)]
    fn random(rng: &mut TestRng) -> Self {
        let rand_digest = Digest::random(rng);
        Self(rand_digest.to_array())
    }
}

/// Message sending direction.
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub(super) enum Role {
    /// Dialer, i.e. initiator of the connection.
    Dialer,
    /// Listener, acceptor of the connection.
    Listener,
}

impl Role {
    #[inline]
    fn in_flag(self) -> u8 {
        !(self.out_flag())
    }

    #[inline]
    fn out_flag(self) -> u8 {
        match self {
            Role::Dialer => 0xaa,
            Role::Listener => !0xaa,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionId, Role, TraceId};

    #[test]
    fn trace_id_has_16_character() {
        let data = [0, 1, 2, 3, 4, 5, 6, 7];

        let output = format!("{}", TraceId(data));

        assert_eq!(output.len(), 16);
    }

    #[test]
    fn can_create_deterministic_trace_id() {
        let mut rng = crate::new_rng();
        let conn_id = ConnectionId::random(&mut rng);

        // TODO: Create proper testcase -- do not require an SSL context to create, then go over
        //       example scenario.

        // let in_0 = conn_id.create_trace_id(Role::Listener.in_flag(), 0);
        // let in_1 = conn_id.create_trace_id(Role::Listener.in_flag(), 1);

        // let out_0 = conn_id.create_trace_id(Role::Dialer.out_flag(), 0);
        // let out_1 = conn_id.create_trace_id(Role::Dialer.out_flag(), 1);
        // let out_2 = conn_id.create_trace_id(Role::Dialer.out_flag(), 2);

        // let in_2 = conn_id.create_trace_id(Role::Listener.in_flag(), 2);

        // // Ensure created IDs are unique.
        // assert_ne!(in_0.0, in_1.0);
        // assert_ne!(in_0.0, in_2.0);
        // assert_ne!(in_1.0, in_2.0);

        // assert_ne!(out_0.0, out_1.0);
        // assert_ne!(out_0.0, out_2.0);
        // assert_ne!(out_1.0, out_2.0);
    }
}
