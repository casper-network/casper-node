use std::{
    fmt::{self, Debug, Display, Formatter},
    io, mem,
    net::SocketAddr,
};

use derive_more::From;
use futures::stream::SplitStream;
use serde::Serialize;
use static_assertions::const_assert;
use tracing::Span;

use super::{
    error::ConnectionError, Error, FramedTransport, GossipedAddress, Message, NodeId, Transport,
};
use crate::{
    effect::{
        announcements::BlocklistAnnouncement,
        requests::{NetworkInfoRequest, NetworkRequest},
    },
    protocol::Message as ProtocolMessage,
};

const _SMALL_NETWORK_EVENT_SIZE: usize = mem::size_of::<Event<ProtocolMessage>>();
const_assert!(_SMALL_NETWORK_EVENT_SIZE < 89);

/// A small network event.
#[derive(Debug, From, Serialize)]
pub enum Event<P> {
    /// The TLS handshake completed on the incoming connection.
    IncomingConnection {
        incoming: Box<IncomingConnection<P>>,
        #[serde(skip)]
        span: Span,
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
        peer_addr: SocketAddr,
        #[serde(skip_serializing)]
        span: Box<Span>,
    },

    /// A new outgoing connection was successfully established.
    OutgoingEstablished {
        remote_addr: SocketAddr,
        peer_id: Box<NodeId>,
        #[serde(skip_serializing)]
        transport: Transport,
    },
    /// An outgoing connection failed to complete dialing.
    OutgoingDialFailure {
        peer_addr: Box<SocketAddr>,
        error: Box<Error>,
    },
    /// An established connection was terminated.
    OutgoingDropped {
        peer_id: Box<NodeId>,
        peer_addr: Box<SocketAddr>,
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
            Event::IncomingConnection { incoming, span: _ } => {
                write!(f, "incoming connection: {}", incoming)
            }
            Event::IncomingMessage {
                peer_id: node_id,
                msg,
            } => write!(f, "msg from {}: {}", node_id, msg),
            Event::IncomingClosed { peer_addr, .. } => {
                write!(f, "closed connection from {}", peer_addr)
            }
            Event::OutgoingEstablished {
                peer_id: node_id, ..
            } => write!(f, "established outgoing to {}", node_id),
            Event::OutgoingDialFailure { peer_addr, error } => {
                write!(f, "outgoing dial failure {}: {}", peer_addr, error)
            }
            Event::OutgoingDropped {
                peer_id,
                peer_addr,
                error,
            } => {
                write!(f, "dropped outgoing {} {}", peer_id, peer_addr)?;
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

/// Outcome of an incoming connection negotiation.
#[derive(Debug, Serialize)]
pub enum IncomingConnection<P> {
    /// The connection failed early on, before even a peer's [`NodeId`] could be determined.
    FailedEarly {
        /// Remote port the peer dialed us from.
        peer_addr: SocketAddr,
        /// Error causing the failure.
        error: ConnectionError,
    },
    /// Connection failed after TLS was successfully established; thus we have a valid [`NodeId`].
    Failed {
        /// Remote port the peer dialed us from.
        peer_addr: SocketAddr,
        /// Peer's [`NodeId`].
        peer_id: NodeId,
        /// Error causing the failure.
        error: ConnectionError,
    },
    /// Connection turned out to be a loopback connection.
    Loopback,
    /// Connection successfully established.
    Established {
        /// Remote port the peer dialed us from.
        peer_addr: SocketAddr,
        /// Public address advertised by the peer.
        public_addr: SocketAddr,
        /// Peer's [`NodeId`].
        peer_id: NodeId,
        /// Stream of incoming messages. for incoming connections.
        #[serde(skip_serializing)]
        stream: SplitStream<FramedTransport<P>>,
    },
}

impl<P> Display for IncomingConnection<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            IncomingConnection::FailedEarly { peer_addr, error } => {
                write!(f, "early failure from {}: {}", peer_addr, error)
            }
            IncomingConnection::Failed {
                peer_addr,
                peer_id,
                error,
            } => write!(f, "failure from {}/{}: {}", peer_addr, peer_id, error),
            IncomingConnection::Loopback => f.write_str("loopback"),
            IncomingConnection::Established {
                peer_addr,
                public_addr,
                peer_id,
                stream: _,
            } => write!(
                f,
                "connection established from {}/{}; public: {}",
                peer_addr, peer_id, public_addr
            ),
        }
    }
}
