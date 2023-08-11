use std::{
    fmt::{self, Debug, Display, Formatter},
    mem,
    net::SocketAddr,
};

use derive_more::From;
use serde::Serialize;
use static_assertions::const_assert;
use tracing::Span;

use casper_types::PublicKey;

use super::{
    error::{ConnectionError, MessageReceiverError},
    GossipedAddress, Message, NodeId, Ticket, Transport,
};
use crate::{
    effect::{
        announcements::PeerBehaviorAnnouncement,
        requests::{NetworkInfoRequest, NetworkRequest},
    },
    protocol::Message as ProtocolMessage,
};

const _NETWORK_EVENT_SIZE: usize = mem::size_of::<Event<ProtocolMessage>>();
const_assert!(_NETWORK_EVENT_SIZE < 999); // TODO: This used to be 65 bytes!

/// A network event.
#[derive(Debug, From, Serialize)]
pub(crate) enum Event<P>
where
    // Note: See notes on the `OutgoingConnection`'s `P: Serialize` trait bound for details.
    P: Serialize,
{
    Initialize,

    /// The TLS handshake completed on the incoming connection.
    IncomingConnection {
        incoming: Box<IncomingConnection>,
        #[serde(skip)]
        span: Span,
    },

    /// Received network message.
    IncomingMessage {
        peer_id: Box<NodeId>,
        msg: Box<Message<P>>,
        #[serde(skip)]
        span: Span,
        /// The backpressure-related ticket for the message.
        #[serde(skip)]
        ticket: Ticket,
    },

    /// Incoming connection closed.
    IncomingClosed {
        #[serde(skip_serializing)]
        result: Result<(), MessageReceiverError>,
        peer_id: Box<NodeId>,
        peer_addr: SocketAddr,
        peer_consensus_public_key: Option<Box<PublicKey>>,
        #[serde(skip_serializing)]
        span: Box<Span>,
    },

    /// A new outgoing connection was successfully established.
    OutgoingConnection {
        outgoing: Box<OutgoingConnection>,
        #[serde(skip_serializing)]
        span: Span,
    },

    /// An established connection was terminated.
    OutgoingDropped {
        peer_id: Box<NodeId>,
        peer_addr: SocketAddr,
    },

    /// Incoming network request.
    #[from]
    NetworkRequest {
        #[serde(skip_serializing)]
        req: Box<NetworkRequest<P>>,
    },

    /// Incoming network info request.
    #[from]
    NetworkInfoRequest {
        #[serde(skip_serializing)]
        req: Box<NetworkInfoRequest>,
    },

    /// The node should gossip its own public listening address.
    GossipOurAddress,

    /// We received a peer's public listening address via gossip.
    PeerAddressReceived(GossipedAddress),

    /// Housekeeping for the outgoing manager.
    SweepOutgoing,

    /// Blocklist announcement.
    #[from]
    BlocklistAnnouncement(PeerBehaviorAnnouncement),
}

impl From<NetworkRequest<ProtocolMessage>> for Event<ProtocolMessage> {
    fn from(req: NetworkRequest<ProtocolMessage>) -> Self {
        Self::NetworkRequest { req: Box::new(req) }
    }
}

impl From<NetworkInfoRequest> for Event<ProtocolMessage> {
    fn from(req: NetworkInfoRequest) -> Self {
        Self::NetworkInfoRequest { req: Box::new(req) }
    }
}

impl<P> Display for Event<P>
where
    P: Display + Serialize,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Initialize => write!(f, "initialize"),
            Event::IncomingConnection { incoming, span: _ } => {
                write!(f, "incoming connection: {}", incoming)
            }
            Event::IncomingMessage {
                peer_id: node_id,
                msg,
                span: _,
                ticket: _,
            } => write!(f, "msg from {}: {}", node_id, msg),
            Event::IncomingClosed { peer_addr, .. } => {
                write!(f, "closed connection from {}", peer_addr)
            }
            Event::OutgoingConnection { outgoing, span: _ } => {
                write!(f, "outgoing connection: {}", outgoing)
            }
            Event::OutgoingDropped { peer_id, peer_addr } => {
                write!(f, "dropped outgoing {} {}", peer_id, peer_addr)
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
        }
    }
}

/// Outcome of an incoming connection negotiation.
// Note: `IncomingConnection` is typically used boxed anyway, so a larget variant is not an issue.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize)]
pub(crate) enum IncomingConnection {
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
        /// The public key the peer is validating with, if any.
        peer_consensus_public_key: Option<Box<PublicKey>>,
        /// Stream of incoming messages. for incoming connections.
        #[serde(skip_serializing)]
        transport: Transport,
    },
}

impl Display for IncomingConnection {
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
                peer_consensus_public_key,
                transport: _,
            } => {
                write!(
                    f,
                    "connection established from {}/{}; public: {}",
                    peer_addr, peer_id, public_addr
                )?;

                if let Some(public_key) = peer_consensus_public_key {
                    write!(f, " [{}]", public_key)
                } else {
                    f.write_str(" [no validator id]")
                }
            }
        }
    }
}

/// Outcome of an outgoing connection attempt.
#[derive(Debug, Serialize)]
pub(crate) enum OutgoingConnection {
    /// The outgoing connection failed early on, before a peer's [`NodeId`] could be determined.
    FailedEarly {
        /// Address that was dialed.
        peer_addr: SocketAddr,
        /// Error causing the failure.
        error: ConnectionError,
    },
    /// Connection failed after TLS was successfully established; thus we have a valid [`NodeId`].
    Failed {
        /// Address that was dialed.
        peer_addr: SocketAddr,
        /// Peer's [`NodeId`].
        peer_id: NodeId,
        /// Error causing the failure.
        error: ConnectionError,
    },
    /// Connection turned out to be a loopback connection.
    Loopback { peer_addr: SocketAddr },
    /// Connection successfully established.
    Established {
        /// Address that was dialed.
        peer_addr: SocketAddr,
        /// Peer's [`NodeId`].
        peer_id: NodeId,
        /// The public key the peer is validating with, if any.
        peer_consensus_public_key: Option<Box<PublicKey>>,
        /// Sink for outgoing messages.
        #[serde(skip)]
        transport: Transport,
    },
}

impl Display for OutgoingConnection {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            OutgoingConnection::FailedEarly { peer_addr, error } => {
                write!(f, "early failure to {}: {}", peer_addr, error)
            }
            OutgoingConnection::Failed {
                peer_addr,
                peer_id,
                error,
            } => write!(f, "failure to {}/{}: {}", peer_addr, peer_id, error),
            OutgoingConnection::Loopback { peer_addr } => write!(f, "loopback to {}", peer_addr),
            OutgoingConnection::Established {
                peer_addr,
                peer_id,
                peer_consensus_public_key,
                transport: _,
            } => {
                write!(f, "connection established to {}/{}", peer_addr, peer_id,)?;

                if let Some(public_key) = peer_consensus_public_key {
                    write!(f, " [{}]", public_key)
                } else {
                    f.write_str(" [no validator id]")
                }
            }
        }
    }
}
