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
    effect::{
        announcements::BlocklistAnnouncement,
        requests::{NetworkInfoRequest, NetworkRequest},
    },
    protocol::Message as ProtocolMessage,
};

const _SMALL_NETWORK_EVENT_SIZE: usize = mem::size_of::<Event<ProtocolMessage>>();
const_assert!(_SMALL_NETWORK_EVENT_SIZE < 89);

#[derive(Debug, From, Serialize)]
pub enum Event<P> {
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
        remote_address: SocketAddr,
        peer_id: Box<NodeId>,
        #[serde(skip_serializing)]
        transport: Transport,
    },
    /// An outgoing connection failed to complete dialing.
    OutgoingDialFailure {
        peer_address: Box<SocketAddr>,
        error: Box<Error>,
    },
    /// An established connection was terminated.
    OutgoingDropped {
        peer_id: Box<NodeId>,
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

    /// We are due for a sweep of the connection symmetries.
    SweepSymmetries,
    /// Housekeeping for the outgoing manager.
    SweepOutgoing,

    /// Blocklist announcement
    #[from]
    BlocklistAnnouncement(BlocklistAnnouncement<NodeId>),
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
            Event::OutgoingDialFailure {
                peer_address,
                error,
            } => {
                write!(f, "outgoing dial failure {}: {}", peer_address, error)
            }
            Event::OutgoingDropped {
                peer_id,
                peer_address,
                error,
            } => {
                write!(f, "dropped outgoing {} {}", peer_id, peer_address)?;
                if let Some(error) = error.as_ref() {
                    write!(f, ": {}", error)?;
                }
                Ok(())
            }
            Event::NetworkRequest { req } => write!(f, "request: {}", req),
            Event::NetworkInfoRequest { req } => write!(f, "request: {}", req),
            Event::GossipOurAddress => write!(f, "gossip our address"),
            Event::PeerAddressReceived(gossiped_address) => {
                write!(f, "received gossiped peer address {}", gossiped_address)
            }
            Event::BlocklistAnnouncement(ann) => {
                write!(f, "handling blocklist announcement: {}", ann)
            }
            Event::SweepOutgoing => {
                write!(f, "sweep outgoing connections")
            }
            Event::SweepSymmetries => {
                write!(f, "sweep connection symmetries")
            }
        }
    }
}
