use std::{
    fmt::{self, Debug, Display, Formatter},
    io, mem,
    net::SocketAddr,
};

use derive_more::From;
use serde::Serialize;
use static_assertions::const_assert;
use tokio::net::TcpStream;

use super::{Error, GossipedAddress, Message, NodeId, Transport};
use crate::{
    effect::requests::{NetworkInfoRequest, NetworkRequest},
    protocol::Message as ProtocolMessage,
};

const _SMALL_NETWORK_EVENT_SIZE: usize = mem::size_of::<Event<ProtocolMessage>>();
const_assert!(_SMALL_NETWORK_EVENT_SIZE < 89);

#[derive(Debug, From, Serialize)]
pub enum Event<P> {
    /// We were isolated and have waited the appropriate time.
    IsolationReconnection,
    /// A new TCP connection has been established from an incoming connection.
    IncomingNew {
        #[serde(skip_serializing)]
        stream: TcpStream,
        peer_address: Box<SocketAddr>,
    },
    /// The TLS handshake completed on the incoming connection.
    IncomingHandshakeCompleted {
        #[serde(skip_serializing)]
        result: Box<Result<(NodeId, Transport), Error>>,
        peer_address: Box<SocketAddr>,
    },
    /// Received network message.
    IncomingMessage {
        peer_id: Box<NodeId>,
        msg: Box<Message<P>>,
    },
    /// Incoming connection closed.
    IncomingClosed {
        #[serde(skip_serializing)]
        result: io::Result<()>,
        peer_id: Box<NodeId>,
        peer_address: Box<SocketAddr>,
    },

    /// A new outgoing connection was successfully established.
    OutgoingEstablished {
        peer_id: Box<NodeId>,
        #[serde(skip_serializing)]
        transport: Transport,
    },
    /// An outgoing connection failed to connect or was terminated.
    OutgoingFailed {
        peer_id: Box<Option<NodeId>>,
        peer_address: Box<SocketAddr>,
        error: Box<Option<Error>>,
    },

    /// Incoming network request.
    #[from]
    NetworkRequest {
        #[serde(skip_serializing)]
        req: Box<NetworkRequest<NodeId, P>>,
    },

    /// Incoming network info request.
    #[from]
    NetworkInfoRequest {
        #[serde(skip_serializing)]
        req: Box<NetworkInfoRequest<NodeId>>,
    },

    /// The node should gossip its own public listening address.
    GossipOurAddress,
    /// We received a peer's public listening address via gossip.
    PeerAddressReceived(GossipedAddress),
}

impl From<NetworkRequest<NodeId, ProtocolMessage>> for Event<ProtocolMessage> {
    fn from(req: NetworkRequest<NodeId, ProtocolMessage>) -> Self {
        Self::NetworkRequest { req: Box::new(req) }
    }
}

impl From<NetworkInfoRequest<NodeId>> for Event<ProtocolMessage> {
    fn from(req: NetworkInfoRequest<NodeId>) -> Self {
        Self::NetworkInfoRequest { req: Box::new(req) }
    }
}

impl<P: Display> Display for Event<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::IsolationReconnection => write!(f, "perform reconnection after isolation"),
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
                peer_id,
                peer_address,
                error,
            } => match &**peer_id {
                Some(node_id) => write!(
                    f,
                    "failed outgoing {} {}: (is_err {})",
                    node_id,
                    peer_address,
                    error.is_some()
                ),
                None => write!(
                    f,
                    "failed outgoing {}: (is_err {})",
                    peer_address,
                    error.is_some()
                ),
            },
            Event::NetworkRequest { req } => write!(f, "request: {}", req),
            Event::NetworkInfoRequest { req } => write!(f, "request: {}", req),
            Event::GossipOurAddress => write!(f, "gossip our address"),
            Event::PeerAddressReceived(gossiped_address) => {
                write!(f, "received gossiped peer address {}", gossiped_address)
            }
        }
    }
}
