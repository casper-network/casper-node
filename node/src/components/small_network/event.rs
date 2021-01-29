use std::{
    fmt::{self, Debug, Display, Formatter},
    io,
    net::SocketAddr,
};

use derive_more::From;
use serde::Serialize;
use tokio::net::TcpStream;

use super::{Error, Message, NodeId, Transport};
use crate::effect::requests::{NetworkInfoRequest, NetworkRequest};

#[derive(Debug, From, Serialize)]
pub enum Event<P> {
    /// Connection to the known node failed.
    BootstrappingFailed {
        peer_address: SocketAddr,
        error: Error,
    },
    /// A new TCP connection has been established from an incoming connection.
    IncomingNew {
        #[serde(skip_serializing)]
        stream: TcpStream,
        peer_address: SocketAddr,
    },
    /// The TLS handshake completed on the incoming connection.
    IncomingHandshakeCompleted {
        #[serde(skip_serializing)]
        result: Result<(NodeId, Transport), Error>,
        peer_address: SocketAddr,
    },
    /// Received network message.
    IncomingMessage { peer_id: NodeId, msg: Message<P> },
    /// Incoming connection closed.
    IncomingClosed {
        #[serde(skip_serializing)]
        result: io::Result<()>,
        peer_id: NodeId,
        peer_address: SocketAddr,
    },

    /// A new outgoing connection was successfully established.
    OutgoingEstablished {
        peer_id: NodeId,
        #[serde(skip_serializing)]
        transport: Transport,
    },
    /// An outgoing connection failed to connect or was terminated.
    OutgoingFailed {
        peer_id: Option<NodeId>,
        peer_address: SocketAddr,
        error: Option<Error>,
    },

    /// Incoming network request.
    #[from]
    NetworkRequest {
        #[serde(skip_serializing)]
        req: NetworkRequest<NodeId, P>,
    },

    /// Incoming network info request.
    #[from]
    NetworkInfoRequest {
        #[serde(skip_serializing)]
        req: NetworkInfoRequest<NodeId>,
    },
}

impl<P: Display> Display for Event<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::BootstrappingFailed {
                peer_address,
                error,
            } => write!(
                f,
                "bootstrapping failed for node {}: {}",
                peer_address, error
            ),
            Event::IncomingNew { peer_address, .. } => {
                write!(f, "incoming connection from {}", peer_address)
            }
            Event::IncomingHandshakeCompleted {
                result,
                peer_address,
            } => write!(
                f,
                "handshake from {}, is_err {}",
                peer_address,
                result.is_err()
            ),
            Event::IncomingMessage {
                peer_id: node_id,
                msg,
            } => write!(f, "msg from {}: {}", node_id, msg),
            Event::IncomingClosed { peer_address, .. } => {
                write!(f, "closed connection from {}", peer_address)
            }
            Event::OutgoingEstablished {
                peer_id: node_id, ..
            } => write!(f, "established outgoing to {}", node_id),
            Event::OutgoingFailed {
                peer_id: Some(node_id),
                peer_address,
                error,
            } => write!(
                f,
                "failed outgoing {} {}: (is_err {})",
                node_id,
                peer_address,
                error.is_some()
            ),
            Event::OutgoingFailed {
                peer_id: None,
                peer_address,
                error,
            } => write!(
                f,
                "failed outgoing {}: (is_err {})",
                peer_address,
                error.is_some()
            ),
            Event::NetworkRequest { req } => write!(f, "request: {}", req),
            Event::NetworkInfoRequest { req } => write!(f, "request: {}", req),
        }
    }
}
