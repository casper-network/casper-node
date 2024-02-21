//! Random unique per-connection ID.
//!
//! This module introduces [`ConnectionId`], a unique ID per established connection that can be
//! independently derived by peers on either side of a connection.

use openssl::ssl::SslRef;
#[cfg(test)]
use rand::RngCore;
use static_assertions::const_assert;

use casper_hashing::Digest;
#[cfg(test)]
use casper_types::testing::TestRng;

use super::tls::KeyFingerprint;
use crate::utils;

/// An ID identifying a connection.
///
/// The ID is guaranteed to be the same on both ends of the connection, unique if at least once side
/// of the connection played "by the rules" and generated a proper nonce.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConnectionId([u8; Digest::LENGTH]);

// Invariant assumed by `ConnectionId`, `Digest` must be <= than `KeyFingerprint`.
const_assert!(KeyFingerprint::LENGTH >= Digest::LENGTH);
// We also assume it is at least 12 bytes.
const_assert!(Digest::LENGTH >= 12);

/// Random data derived from TLS connections.
#[derive(Copy, Clone, Debug)]
pub(super) struct TlsRandomData {
    /// Random data extracted from the client of the connection.
    digest: Digest,
}

/// Length of the TLS-derived random data.
const RLEN: usize = 32;

impl TlsRandomData {
    /// Collects random data from an existing SSL collection.
    fn collect(ssl: &SslRef) -> Self {
        // Both server random and client random are public, we just need ours to be truly random for
        // security reasons.
        let mut combined_random: [u8; RLEN * 2] = [0; RLEN * 2];

        // Combine both. Important: Assume an attacker knows one of these ahead of time, due to the
        // way TLS handshakes work.
        ssl.server_random(&mut combined_random[0..RLEN]);
        ssl.client_random(&mut combined_random[RLEN..]);

        Self {
            digest: Digest::hash(&combined_random),
        }
    }

    /// Creates random `TlsRandomData`.
    #[cfg(test)]
    fn random(rng: &mut TestRng) -> Self {
        let mut buffer = [0u8; RLEN * 2];

        rng.fill_bytes(&mut buffer);

        Self {
            digest: Digest::hash(&buffer),
        }
    }
}

impl ConnectionId {
    /// Creates a new connection ID, based on random values from server and client and a prefix.
    fn create(random_data: TlsRandomData) -> ConnectionId {
        // Just to be sure, create a prefix and hash again.
        // TODO: Consider replacing with a key derivation function instead.
        const PREFIX: &[u8] = b"CONNECTION_ID//";
        const TOTAL_LEN: usize = PREFIX.len() + Digest::LENGTH;

        let mut data = [0; TOTAL_LEN];
        let (data_prefix, data_suffix) = &mut data[..].split_at_mut(PREFIX.len());

        data_prefix.copy_from_slice(PREFIX);
        data_suffix.copy_from_slice(&random_data.digest.value());

        let id = Digest::hash(data).value();

        ConnectionId(id)
    }

    #[inline]
    /// Returns a reference to the raw bytes of the connection ID.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Creates a new connection ID from an existing SSL connection.
    #[inline]
    pub(crate) fn from_connection(ssl: &SslRef) -> Self {
        Self::create(TlsRandomData::collect(ssl))
    }

    /// Creates a random `ConnectionId`.
    #[cfg(test)]
    pub(super) fn random(rng: &mut TestRng) -> Self {
        ConnectionId::create(TlsRandomData::random(rng))
    }
}
