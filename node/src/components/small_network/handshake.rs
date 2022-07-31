//! Handshake handling for `small_network`.
//!
//! The handshake differs from the rest of the networking code since it is (almost) unmodified since
//! version 1.0, to allow nodes to make informed decisions about blocking other nodes.

use std::{error::Error as StdError, net::SocketAddr, time::Duration};

use casper_types::PublicKey;
use futures::Future;

use super::{
    counting_format::ConnectionId,
    error::{ConnectionError, IoError},
    message_pack_format::MessagePackFormat,
    tasks::NetworkContext,
    Payload, Transport,
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

/// Performs an IO-operation that can time out.
pub(super) async fn io_timeout<F, T, E>(duration: Duration, future: F) -> Result<T, IoError<E>>
where
    F: Future<Output = Result<T, E>>,
    E: StdError + 'static,
{
    tokio::time::timeout(duration, future)
        .await
        .map_err(|_elapsed| IoError::Timeout)?
        .map_err(IoError::Error)
}

/// Performs an IO-operation that can time out or result in a closed connection.
pub(super) async fn io_opt_timeout<F, T, E>(duration: Duration, future: F) -> Result<T, IoError<E>>
where
    F: Future<Output = Option<Result<T, E>>>,
    E: StdError + 'static,
{
    let item = tokio::time::timeout(duration, future)
        .await
        .map_err(|_elapsed| IoError::Timeout)?;

    match item {
        Some(Ok(value)) => Ok(value),
        Some(Err(err)) => Err(IoError::Error(err)),
        None => Err(IoError::UnexpectedEof),
    }
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
    let mut encoder = MessagePackFormat;

    // // Manually encode a handshake.
    // let handshake_message = context.chain_info.create_handshake::<P>(
    //     context.public_addr,
    //     context.consensus_keys.as_ref(),
    //     connection_id,
    //     context.is_syncing.load(Ordering::SeqCst),
    // );

    // let serialized_handshake_message = Pin::new(&mut encoder)
    //     .serialize(&Arc::new(handshake_message))
    //     .map_err(ConnectionError::CouldNotEncodeOurHandshake)?;

    // // To ensure we are not dead-locking, we split the framed transport here and send the handshake
    // // in a background task before awaiting one ourselves. This ensures we can make progress
    // // regardless of the size of the outgoing handshake.
    // let (mut sink, mut stream) = framed.split();

    // let handshake_send = tokio::spawn(io_timeout(context.handshake_timeout.into(), async move {
    //     sink.send(serialized_handshake_message).await?;
    //     Ok(sink)
    // }));

    // // The remote's message should be a handshake, but can technically be any message. We receive,
    // // deserialize and check it.
    // let remote_message_raw = io_opt_timeout(context.handshake_timeout.into(), stream.next())
    //     .await
    //     .map_err(ConnectionError::HandshakeRecv)?;

    // // Ensure the handshake was sent correctly.
    // let sink = handshake_send
    //     .await
    //     .map_err(ConnectionError::HandshakeSenderCrashed)?
    //     .map_err(ConnectionError::HandshakeSend)?;

    // let remote_message: Message<P> = Pin::new(&mut encoder)
    //     .deserialize(&remote_message_raw)
    //     .map_err(ConnectionError::InvalidRemoteHandshakeMessage)?;

    // if let Message::Handshake {
    //     network_name,
    //     public_addr,
    //     protocol_version,
    //     consensus_certificate,
    //     is_syncing,
    //     chainspec_hash,
    // } = remote_message
    // {
    //     debug!(%protocol_version, "handshake received");

    //     // The handshake was valid, we can check the network name.
    //     if network_name != context.chain_info.network_name {
    //         return Err(ConnectionError::WrongNetwork(network_name));
    //     }

    //     // If there is a version mismatch, we treat it as a connection error. We do not ban peers
    //     // for this error, but instead rely on exponential backoff, as bans would result in issues
    //     // during upgrades where nodes may have a legitimate reason for differing versions.
    //     //
    //     // Since we are not using SemVer for versioning, we cannot make any assumptions about
    //     // compatibility, so we allow only exact version matches.
    //     if protocol_version != context.chain_info.protocol_version {
    //         if let Some(threshold) = context.tarpit_version_threshold {
    //             if protocol_version <= threshold {
    //                 let mut rng = crate::new_rng();

    //                 if rng.gen_bool(context.tarpit_chance as f64) {
    //                     // If tarpitting is enabled, we hold open the connection for a specific
    //                     // amount of time, to reduce load on other nodes and keep them from
    //                     // reconnecting.
    //                     info!(duration=?context.tarpit_duration, "randomly tarpitting node");
    //                     tokio::time::sleep(Duration::from(context.tarpit_duration)).await;
    //                 } else {
    //                     debug!(p = context.tarpit_chance, "randomly not tarpitting node");
    //                 }
    //             }
    //         }
    //         return Err(ConnectionError::IncompatibleVersion(protocol_version));
    //     }

    //     // We check the chainspec hash to ensure peer is using the same chainspec as us.
    //     // The remote message should always have a chainspec hash at this point since
    //     // we checked the protocol version previously.
    //     let peer_chainspec_hash = chainspec_hash.ok_or(ConnectionError::MissingChainspecHash)?;
    //     if peer_chainspec_hash != context.chain_info.chainspec_hash {
    //         return Err(ConnectionError::WrongChainspecHash(peer_chainspec_hash));
    //     }

    //     let peer_consensus_public_key = consensus_certificate
    //         .map(|cert| {
    //             cert.validate(connection_id)
    //                 .map_err(ConnectionError::InvalidConsensusCertificate)
    //         })
    //         .transpose()?;

    //     let framed_transport = sink
    //         .reunite(stream)
    //         .map_err(|_| ConnectionError::FailedToReuniteHandshakeSinkAndStream)?;

    //     Ok(HandshakeOutcome {
    //         framed_transport,
    //         public_addr,
    //         peer_consensus_public_key,
    //         is_peer_syncing: is_syncing,
    //     })
    // } else {
    //     // Received a non-handshake, this is an error.
    //     Err(ConnectionError::DidNotSendHandshake)
    // }

    todo!()
}
