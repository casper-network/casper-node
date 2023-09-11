//! Observability for network serialization/deserialization.
//!
//! This module introduces [`ConnectionId`], a unique ID per established connection that can be
//! independently derived by peers on either side of a connection.

use openssl::ssl::SslRef;
#[cfg(test)]
use rand::RngCore;
use static_assertions::const_assert;
use tracing::warn;

use casper_hashing::Digest;
#[cfg(test)]
use casper_types::testing::TestRng;

use super::tls::KeyFingerprint;
use crate::{types::NodeId, utils};

/// An ID identifying a connection.
///
/// The ID is guaranteed to be the same on both ends of the connection, but not guaranteed to be
/// unique or sufficiently random. Do not use it for any cryptographic/security related purposes.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
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

/// Zero-randomness.
///
/// Used to check random data.
const ZERO_RANDOMNESS: [u8; 12] = [0; 12];

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

        if server_random == ZERO_RANDOMNESS {
            warn!("TLS server random is all zeros");
        }

        ssl.client_random(&mut client_random);

        if client_random == ZERO_RANDOMNESS {
            warn!("TLS client random is all zeros");
        }

        // Combine using XOR.
        utils::xor(&mut server_random, &client_random);

        Self {
            combined_random: server_random,
        }
    }

    /// Creates random `TlsRandomData`.
    #[cfg(test)]
    fn random(rng: &mut TestRng) -> Self {
        let mut buffer = [0u8; 12];

        rng.fill_bytes(&mut buffer);

        Self {
            combined_random: buffer,
        }
    }
}

impl ConnectionId {
    /// Creates a new connection ID, based on random values from server and client, as well as
    /// node IDs.
    fn create(random_data: TlsRandomData, our_id: NodeId, their_id: NodeId) -> ConnectionId {
        // Hash the resulting random values.
        let mut id = Digest::hash(random_data.combined_random).value();

        // We XOR in a hashes of server and client fingerprint, to ensure that in the case of an
        // accidental collision (e.g. when `server_random` and `client_random` turn out to be all
        // zeros), we still have a chance of producing a reasonable ID.
        utils::xor(&mut id, &our_id.hash_bytes()[0..Digest::LENGTH]);
        utils::xor(&mut id, &their_id.hash_bytes()[0..Digest::LENGTH]);

        ConnectionId(id)
    }

    #[inline]
    /// Returns a reference to the raw bytes of the connection ID.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Creates a new connection ID from an existing SSL connection.
    #[inline]
    pub(crate) fn from_connection(ssl: &SslRef, our_id: NodeId, their_id: NodeId) -> Self {
        Self::create(TlsRandomData::collect(ssl), our_id, their_id)
    }

    /// Creates a random `ConnectionId`.
    #[cfg(test)]
    pub(super) fn random(rng: &mut TestRng) -> Self {
        ConnectionId::create(
            TlsRandomData::random(rng),
            NodeId::random(rng),
            NodeId::random(rng),
        )
    }
}
