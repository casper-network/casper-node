//! Tasks run by the component.

use std::{fmt::Display, io, net::SocketAddr, pin::Pin, sync::Arc};

use anyhow::Context;

use futures::{
    future::{self, Either},
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use openssl::{
    pkey::{PKey, Private},
    ssl::Ssl,
};
use prometheus::IntGauge;
use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    net::TcpStream,
    sync::{mpsc::UnboundedReceiver, watch},
};
use tokio_openssl::SslStream;
use tracing::{debug, info, warn};

use crate::{
    reactor::{EventQueueHandle, QueueKind},
    tls::{self, TlsCert},
    types::NodeId,
};

use super::{
    error::{display_error, Error, Result},
    Event, FramedTransport, Message, Payload, Transport,
};

/// Server-side TLS handshake.
///
/// This function groups the TLS handshake into a convenient function, enabling the `?` operator.
pub(super) async fn setup_tls(
    stream: TcpStream,
    cert: Arc<TlsCert>,
    secret_key: Arc<PKey<Private>>,
) -> Result<(NodeId, Transport)> {
    let mut tls_stream = tls::create_tls_acceptor(&cert.as_x509().as_ref(), &secret_key.as_ref())
        .and_then(|ssl_acceptor| Ssl::new(ssl_acceptor.context()))
        .and_then(|ssl| SslStream::new(ssl, stream))
        .map_err(Error::AcceptorCreation)?;

    SslStream::accept(Pin::new(&mut tls_stream))
        .await
        .map_err(Error::Handshake)?;

    // We can now verify the certificate.
    let peer_cert = tls_stream
        .ssl()
        .peer_certificate()
        .ok_or(Error::NoClientCertificate)?;

    Ok((
        NodeId::from(tls::validate_cert(peer_cert)?.public_key_fingerprint()),
        tls_stream,
    ))
}

/// Network handshake reader for single handshake message received by outgoing connection.
pub(super) async fn handshake_reader<REv, P>(
    event_queue: EventQueueHandle<REv>,
    mut stream: SplitStream<FramedTransport<P>>,
    our_id: NodeId,
    peer_id: NodeId,
    peer_address: SocketAddr,
) where
    P: DeserializeOwned + Send + Display + Payload,
    REv: From<Event<P>>,
{
    if let Some(Ok(msg @ Message::Handshake { .. })) = stream.next().await {
        debug!(%our_id, %msg, %peer_id, "handshake received");
        return event_queue
            .schedule(
                Event::IncomingMessage {
                    peer_id: Box::new(peer_id),
                    msg: Box::new(msg),
                },
                QueueKind::NetworkIncoming,
            )
            .await;
    }
    warn!(%our_id, %peer_id, "receiving handshake failed, closing connection");
    event_queue
        .schedule(
            Event::OutgoingDropped {
                peer_id: Box::new(peer_id),
                peer_address: Box::new(peer_address),
                error: Box::new(None),
            },
            QueueKind::Network,
        )
        .await
}

/// Initiates a TLS connection to a remote address.
pub(super) async fn connect_outgoing(
    peer_address: SocketAddr,
    our_certificate: Arc<TlsCert>,
    secret_key: Arc<PKey<Private>>,
) -> Result<(NodeId, Transport)> {
    let ssl = tls::create_tls_connector(&our_certificate.as_x509(), &secret_key)
        .context("could not create TLS connector")?
        .configure()
        .and_then(|mut config| {
            config.set_verify_hostname(false);
            config.into_ssl("this-will-not-be-checked.example.com")
        })
        .map_err(Error::ConnectorConfiguration)?;

    let stream = TcpStream::connect(peer_address)
        .await
        .context("TCP connection failed")?;

    let mut tls_stream = SslStream::new(ssl, stream).context("tls handshake failed")?;
    SslStream::connect(Pin::new(&mut tls_stream)).await?;

    let peer_cert = tls_stream
        .ssl()
        .peer_certificate()
        .ok_or(Error::NoServerCertificate)?;

    let peer_id = tls::validate_cert(peer_cert)?.public_key_fingerprint();

    Ok((NodeId::from(peer_id), tls_stream))
}

/// Core accept loop for the networking server.
///
/// Never terminates.
pub(super) async fn server<P, REv>(
    event_queue: EventQueueHandle<REv>,
    listener: tokio::net::TcpListener,
    mut shutdown_receiver: watch::Receiver<()>,
    our_id: NodeId,
) where
    REv: From<Event<P>>,
{
    // The server task is a bit tricky, since it has to wait on incoming connections while at the
    // same time shut down if the networking component is dropped, otherwise the TCP socket will
    // stay open, preventing reuse.

    // We first create a future that never terminates, handling incoming connections:
    let accept_connections = async move {
        loop {
            // We handle accept errors here, since they can be caused by a temporary resource
            // shortage or the remote side closing the connection while it is waiting in
            // the queue.
            match listener.accept().await {
                Ok((stream, peer_address)) => {
                    // Move the incoming connection to the event queue for handling.
                    let event = Event::IncomingNew {
                        stream,
                        peer_address: Box::new(peer_address),
                    };
                    event_queue
                        .schedule(event, QueueKind::NetworkIncoming)
                        .await;
                }
                // TODO: Handle resource errors gracefully.
                //       In general, two kinds of errors occur here: Local resource exhaustion,
                //       which should be handled by waiting a few milliseconds, or remote connection
                //       errors, which can be dropped immediately.
                //
                //       The code in its current state will consume 100% CPU if local resource
                //       exhaustion happens, as no distinction is made and no delay introduced.
                Err(ref err) => {
                    warn!(%our_id, err=display_error(err), "dropping incoming connection during accept")
                }
            }
        }
    };

    let shutdown_messages = async move { while shutdown_receiver.changed().await.is_ok() {} };

    // Now we can wait for either the `shutdown` channel's remote end to do be dropped or the
    // infinite loop to terminate, which never happens.
    match future::select(Box::pin(shutdown_messages), Box::pin(accept_connections)).await {
        Either::Left(_) => info!(
            %our_id,
            "shutting down socket, no longer accepting incoming connections"
        ),
        Either::Right(_) => unreachable!(),
    }
}

/// Network message reader.
///
/// Schedules all received messages until the stream is closed or an error occurs.
pub(super) async fn message_reader<REv, P>(
    event_queue: EventQueueHandle<REv>,
    mut stream: SplitStream<FramedTransport<P>>,
    mut shutdown_receiver: watch::Receiver<()>,
    our_id: NodeId,
    peer_id: NodeId,
) -> io::Result<()>
where
    P: DeserializeOwned + Send + Display + Payload,
    REv: From<Event<P>>,
{
    let read_messages = async move {
        while let Some(msg_result) = stream.next().await {
            match msg_result {
                Ok(msg) => {
                    debug!(%our_id, %msg, %peer_id, "message received");
                    // We've received a message, push it to the reactor.
                    event_queue
                        .schedule(
                            Event::IncomingMessage {
                                peer_id: Box::new(peer_id),
                                msg: Box::new(msg),
                            },
                            QueueKind::NetworkIncoming,
                        )
                        .await;
                }
                Err(err) => {
                    warn!(%our_id, err=display_error(&err), %peer_id, "receiving message failed, closing connection");
                    return Err(err);
                }
            }
        }
        Ok(())
    };

    let shutdown_messages = async move { while shutdown_receiver.changed().await.is_ok() {} };

    // Now we can wait for either the `shutdown` channel's remote end to do be dropped or the
    // while loop to terminate.
    match future::select(Box::pin(shutdown_messages), Box::pin(read_messages)).await {
        Either::Left(_) => info!(
            %our_id,
            %peer_id,
            "shutting down incoming connection message reader"
        ),
        Either::Right(_) => (),
    }

    Ok(())
}

/// Network message sender.
///
/// Reads from a channel and sends all messages, until the stream is closed or an error occurs.
///
/// Initially sends a handshake including the `chainspec_hash` as a final handshake step.  If the
/// recipient's `chainspec_hash` doesn't match, the connection will be closed.
pub(super) async fn message_sender<P>(
    mut queue: UnboundedReceiver<Message<P>>,
    mut sink: SplitSink<FramedTransport<P>, Message<P>>,
    counter: IntGauge,
    handshake: Message<P>,
) -> Result<()>
where
    P: Serialize + Send + Payload,
{
    sink.send(handshake).await.map_err(Error::MessageNotSent)?;
    while let Some(payload) = queue.recv().await {
        counter.dec();
        // We simply error-out if the sink fails, it means that our connection broke.
        sink.send(payload).await.map_err(Error::MessageNotSent)?;
    }

    Ok(())
}
