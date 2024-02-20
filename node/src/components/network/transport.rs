//! Low-level network transport configuration.
//!
//! The low-level transport is built on top of an existing TLS stream, handling all multiplexing. It
//! is based on a configuration of the Juliet protocol implemented in the `juliet` crate.

use std::pin::Pin;

use casper_types::TimeDiff;
use juliet::rpc::IncomingRequest;
use openssl::ssl::Ssl;
use strum::EnumCount;
use tokio::net::TcpStream;
use tokio_openssl::SslStream;

use crate::{
    tls,
    types::{chainspec::JulietConfig, NodeId},
};

use super::{
    conman::{ProtocolHandler, ProtocolHandshakeOutcome},
    error::ConnectionError,
    tasks::TlsConfiguration,
    Channel, PerChannel, Transport,
};

/// Creats a new RPC builder with the currently fixed Juliet configuration.
///
/// The resulting `RpcBuilder` can be reused for multiple connections.
pub(super) fn create_rpc_builder(
    juliet_config: PerChannel<JulietConfig>,
    buffer_size: PerChannel<Option<usize>>,
    ack_timeout: TimeDiff,
) -> juliet::rpc::RpcBuilder<{ Channel::COUNT }> {
    let protocol = juliet_config.into_iter().fold(
        juliet::protocol::ProtocolBuilder::new(),
        |protocol, (channel, juliet_config)| {
            protocol.channel_config(channel.into_channel_id(), juliet_config.into())
        },
    );

    // If buffer_size is not specified, `in_flight_limit * 2` is used:
    let buffer_size = buffer_size.map(|channel, maybe_buffer_size| {
        maybe_buffer_size.unwrap_or((2 * juliet_config.get(channel).in_flight_limit).into())
    });

    let io_core = buffer_size.into_iter().fold(
        juliet::io::IoCoreBuilder::new(protocol),
        |io_core, (channel, buffer_size)| {
            io_core.buffer_size(channel.into_channel_id(), buffer_size)
        },
    );

    juliet::rpc::RpcBuilder::new(io_core)
        .with_bubble_timeouts(true)
        .with_default_timeout(ack_timeout.into())
}

/// Adapter for incoming Juliet requests.
///
/// At this time the node does not take full advantage of the Juliet RPC capabilities, relying on
/// its older message+ACK based model introduced with `muxink`. In this model, every message is only
/// acknowledged, with no request-response association being done. The ACK indicates that the peer
/// is free to send another message.
///
/// The [`Ticket`] type is used to track the processing of an incoming message or its resulting
/// operations; it should dropped once the resources for doing so have been spent, but no earlier.
///
/// Dropping it will cause an "ACK", which in the Juliet transport's case is an empty response, to
/// be sent. Cancellations or responses with actual payloads are not used at this time.
#[derive(Debug)]
pub(crate) struct Ticket(Option<Box<IncomingRequest>>);

impl Ticket {
    #[inline(always)]
    pub(super) fn from_rpc_request(incoming_request: IncomingRequest) -> Self {
        Ticket(Some(Box::new(incoming_request)))
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn create_dummy() -> Self {
        Ticket(None)
    }
}

impl Drop for Ticket {
    #[inline(always)]
    fn drop(&mut self) {
        // Currently, we simply send a request confirmation in the for of an `ACK`.
        if let Some(incoming_request) = self.0.take() {
            incoming_request.respond(None);
        }
    }
}

pub(super) struct ComponentProtocolHandler {
    tls_configuration: TlsConfiguration,
}

impl ComponentProtocolHandler {
    pub(super) fn new() -> Self {
        todo!()
    }
}

#[async_trait::async_trait]
impl ProtocolHandler for ComponentProtocolHandler {
    #[inline(always)]
    async fn setup_incoming(
        &self,
        stream: TcpStream,
    ) -> Result<ProtocolHandshakeOutcome, ConnectionError> {
        let (node_id, transport) = server_setup_tls(&self.tls_configuration, stream).await?;

        todo!()
    }

    #[inline(always)]
    async fn setup_outgoing(
        &self,
        stream: TcpStream,
    ) -> Result<ProtocolHandshakeOutcome, ConnectionError> {
        todo!()
    }

    fn handle_incoming_request(&self, peer: NodeId, request: IncomingRequest) {
        todo!()
    }
}

/// Server-side TLS setup.
///
/// This function groups the TLS setup into a convenient function, enabling the `?` operator.
pub(super) async fn server_setup_tls(
    context: &TlsConfiguration,
    stream: TcpStream,
) -> Result<(NodeId, Transport), ConnectionError> {
    let mut tls_stream = tls::create_tls_acceptor(
        context.our_cert.as_x509().as_ref(),
        context.secret_key.as_ref(),
        context.keylog.clone(),
    )
    .and_then(|ssl_acceptor| Ssl::new(ssl_acceptor.context()))
    .and_then(|ssl| SslStream::new(ssl, stream))
    .map_err(ConnectionError::TlsInitialization)?;

    SslStream::accept(Pin::new(&mut tls_stream))
        .await
        .map_err(ConnectionError::TlsHandshake)?;

    // We can now verify the certificate.
    let peer_cert = tls_stream
        .ssl()
        .peer_certificate()
        .ok_or(ConnectionError::NoPeerCertificate)?;

    let validated_peer_cert = context
        .validate_peer_cert(peer_cert)
        .map_err(ConnectionError::PeerCertificateInvalid)?;

    Ok((
        NodeId::from(validated_peer_cert.public_key_fingerprint()),
        tls_stream,
    ))
}
