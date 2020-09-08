use std::{
    fmt::{self, Debug, Display, Formatter},
    io,
    net::SocketAddr,
};

use derive_more::From;
use tokio::net::TcpStream;

use super::{Error, GossipedAddress, Message, NodeId, Transport};
use crate::effect::requests::NetworkRequest;

#[derive(Debug, From)]
pub enum Event<P> {
    /// Connection to the known node failed.
    BootstrappingFailed { address: SocketAddr, error: Error },
    /// A new TCP connection has been established from an incoming connection.
    IncomingNew {
        stream: TcpStream,
        address: SocketAddr,
    },
    /// The TLS handshake completed on the incoming connection.
    IncomingHandshakeCompleted {
        result: Result<(NodeId, Transport), Error>,
        address: SocketAddr,
    },
    /// Received network message.
    IncomingMessage { peer_id: NodeId, msg: Message<P> },
    /// Incoming connection closed.
    IncomingClosed {
        result: io::Result<()>,
        peer_id: NodeId,
        address: SocketAddr,
    },

    /// A new outgoing connection was successfully established.
    OutgoingEstablished {
        peer_id: NodeId,
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
    NetworkRequest { req: NetworkRequest<NodeId, P> },

    /// The node should gossip its own public listening address.
    GossipOurAddress,
    /// We received a peer's public listening address via gossip.
    PeerAddressReceived(GossipedAddress),
}

impl<P: Display> Display for Event<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::BootstrappingFailed { address, error } => {
                write!(f, "bootstrapping failed for node {}: {}", address, error)
            }
            Event::IncomingNew { address, .. } => write!(f, "incoming connection from {}", address),
            Event::IncomingHandshakeCompleted { result, address } => {
                write!(f, "handshake from {}, is_err {}", address, result.is_err())
            }
            Event::IncomingMessage {
                peer_id: node_id,
                msg,
            } => write!(f, "msg from {}: {}", node_id, msg),
            Event::IncomingClosed { address, .. } => {
                write!(f, "closed connection from {}", address)
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
            Event::GossipOurAddress => write!(f, "gossip our address"),
            Event::PeerAddressReceived(gossiped_address) => {
                write!(f, "received gossiped peer address {}", gossiped_address)
            }
        }
    }
}
