//! Handshake handling for `small_network`.
//!
//! The handshake differs from the rest of the networking code since it is (almost) unmodified since
//! version 1.0, to allow nodes to make informed decisions about blocking other nodes.
//!
//! This module contains an implementation for a minimal framing format based on 32-bit fixed size
//! big endian length prefixes.

use std::{net::SocketAddr, sync::atomic::Ordering, time::Duration};

use casper_types::PublicKey;
use rand::Rng;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use serde::{de::DeserializeOwned, Serialize};
use tracing::{debug, info};

use super::{
    counting_format::ConnectionId,
    error::{ConnectionError, RawFrameIoError},
    tasks::NetworkContext,
    Message, Payload, Transport,
};

/// The outcome of the handshake process.
pub(super) struct HandshakeOutcome {
    /// A framed transport for peer.
    pub(super) transport: Transport,
    /// Public address advertised by the peer.
    pub(super) public_addr: SocketAddr,
    /// The public key the peer is validating with, if any.
    pub(super) peer_consensus_public_key: Option<PublicKey>,
    /// Holds the information whether the remote node is syncing.
    pub(super) is_peer_syncing: bool,
}

/// Reads a 32 byte big endian integer prefix, followed by an actual raw message.
async fn read_length_prefixed_frame<R>(
    max_length: u32,
    stream: &mut R,
) -> Result<Vec<u8>, RawFrameIoError>
where
    R: AsyncRead + Unpin,
{
    let mut length_prefix_raw: [u8; 4] = [0; 4];
    stream
        .read_exact(&mut length_prefix_raw)
        .await
        .map_err(RawFrameIoError::Io)?;

    let length = u32::from_ne_bytes(length_prefix_raw);

    if length > max_length {
        return Err(RawFrameIoError::MaximumLengthExceeded(length as usize));
    }

    let mut raw = Vec::new(); // not preallocating, to make DOS attacks harder.

    // We can now read the raw frame and return.
    stream
        .take(length as u64)
        .read_to_end(&mut raw)
        .await
        .map_err(RawFrameIoError::Io)?;

    Ok(raw)
}

/// Writes data to an async writer, prefixing it with the 32 bytes big endian message length.
///
/// Output will be flushed after sending.
async fn write_length_prefixed_frame<W>(stream: &mut W, data: &[u8]) -> Result<(), RawFrameIoError>
where
    W: AsyncWrite + Unpin,
{
    if data.len() > u32::MAX as usize {
        return Err(RawFrameIoError::MaximumLengthExceeded(data.len()));
    }

    async move {
        stream.write_all(&(data.len() as u32).to_ne_bytes()).await?;
        stream.write_all(&data).await?;
        stream.flush().await?;
        Ok(())
    }
    .await
    .map_err(RawFrameIoError::Io)?;

    Ok(())
}

/// Serializes an item with the encoding settings specified for handshakes.
pub(crate) fn serialize<T>(item: &T) -> Result<Vec<u8>, rmp_serde::encode::Error>
where
    T: Serialize,
{
    rmp_serde::to_vec(item)
}

/// Deserialize an item with the encoding settings specified for handshakes.
pub(crate) fn deserialize<T>(raw: &[u8]) -> Result<T, rmp_serde::decode::Error>
where
    T: DeserializeOwned,
{
    rmp_serde::from_slice(raw)
}

/// Negotiates a handshake between two peers.
pub(super) async fn negotiate_handshake<P, REv>(
    context: &NetworkContext<REv>,
    transport: Transport,
    connection_id: ConnectionId,
) -> Result<HandshakeOutcome, ConnectionError>
where
    P: Payload,
{
    // Manually encode a handshake.
    let handshake_message = context.chain_info.create_handshake::<P>(
        context.public_addr,
        context.consensus_keys.as_ref(),
        connection_id,
        context.is_syncing.load(Ordering::SeqCst),
    );

    let serialized_handshake_message =
        serialize(&handshake_message).map_err(ConnectionError::CouldNotEncodeOurHandshake)?;

    // To ensure we are not dead-locking, we split the transport here and send the handshake in a
    // background task before awaiting one ourselves. This ensures we can make progress regardless
    // of the size of the outgoing handshake.
    let (mut read_half, mut write_half) = tokio::io::split(transport);

    let handshake_send = tokio::spawn(async move {
        write_length_prefixed_frame(&mut write_half, &serialized_handshake_message).await?;
        Ok::<_, RawFrameIoError>(write_half)
    });

    // The remote's message should be a handshake, but can technically be any message. We receive,
    // deserialize and check it.
    let remote_message_raw =
        read_length_prefixed_frame(context.chain_info.maximum_net_message_size, &mut read_half)
            .await
            .map_err(ConnectionError::HandshakeRecv)?;

    // Ensure the handshake was sent correctly.
    let write_half = handshake_send
        .await
        .map_err(ConnectionError::HandshakeSenderCrashed)?
        .map_err(ConnectionError::HandshakeSend)?;

    let remote_message: Message<P> =
        deserialize(&remote_message_raw).map_err(ConnectionError::InvalidRemoteHandshakeMessage)?;

    if let Message::Handshake {
        network_name,
        public_addr,
        protocol_version,
        consensus_certificate,
        is_syncing,
        chainspec_hash,
    } = remote_message
    {
        debug!(%protocol_version, "handshake received");

        // The handshake was valid, we can check the network name.
        if network_name != context.chain_info.network_name {
            return Err(ConnectionError::WrongNetwork(network_name));
        }

        // If there is a version mismatch, we treat it as a connection error. We do not ban peers
        // for this error, but instead rely on exponential backoff, as bans would result in issues
        // during upgrades where nodes may have a legitimate reason for differing versions.
        //
        // Since we are not using SemVer for versioning, we cannot make any assumptions about
        // compatibility, so we allow only exact version matches.
        if protocol_version != context.chain_info.protocol_version {
            if let Some(threshold) = context.tarpit_version_threshold {
                if protocol_version <= threshold {
                    let mut rng = crate::new_rng();

                    if rng.gen_bool(context.tarpit_chance as f64) {
                        // If tarpitting is enabled, we hold open the connection for a specific
                        // amount of time, to reduce load on other nodes and keep them from
                        // reconnecting.
                        info!(duration=?context.tarpit_duration, "randomly tarpitting node");
                        tokio::time::sleep(Duration::from(context.tarpit_duration)).await;
                    } else {
                        debug!(p = context.tarpit_chance, "randomly not tarpitting node");
                    }
                }
            }
            return Err(ConnectionError::IncompatibleVersion(protocol_version));
        }

        // We check the chainspec hash to ensure peer is using the same chainspec as us.
        // The remote message should always have a chainspec hash at this point since
        // we checked the protocol version previously.
        let peer_chainspec_hash = chainspec_hash.ok_or(ConnectionError::MissingChainspecHash)?;
        if peer_chainspec_hash != context.chain_info.chainspec_hash {
            return Err(ConnectionError::WrongChainspecHash(peer_chainspec_hash));
        }

        let peer_consensus_public_key = consensus_certificate
            .map(|cert| {
                cert.validate(connection_id)
                    .map_err(ConnectionError::InvalidConsensusCertificate)
            })
            .transpose()?;

        let transport = read_half.unsplit(write_half);

        Ok(HandshakeOutcome {
            transport,
            public_addr,
            peer_consensus_public_key,
            is_peer_syncing: is_syncing,
        })
    } else {
        // Received a non-handshake, this is an error.
        Err(ConnectionError::DidNotSendHandshake)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn frame_reader_reads_without_consuming_extra_bytes() {
        todo!("implement test");
    }

    #[test]
    fn frame_reader_does_not_allow_exceeding_maximum_size() {
        todo!("implement test");
    }

    #[test]
    fn frame_reader_handles_0_sized_read() {
        todo!("implement test");
    }

    #[test]
    fn frame_reader_handles_early_eof() {
        todo!("implement test");
    }

    #[test]
    fn frame_writer_writes_frames_correctly() {
        todo!("implement test");
    }

    #[test]
    fn frame_writer_handles_0_size() {
        todo!("implement test");
    }
}
