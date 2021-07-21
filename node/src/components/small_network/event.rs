use std::{
    collections::HashSet,
    fmt::{self, Debug, Display, Formatter},
    io, mem,
    net::SocketAddr,
    sync::Arc,
};

use casper_types::PublicKey;
use derive_more::From;
use futures::stream::{SplitSink, SplitStream};
use serde::Serialize;
use static_assertions::const_assert;
use tracing::Span;

use super::{error::ConnectionError, FramedTransport, GossipedAddress, Message, NodeId};
use crate::{
    effect::{
        announcements::{BlocklistAnnouncement, LinearChainAnnouncement},
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
        #[serde(skip)]
        span: Span,
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
    OutgoingConnection {
        outgoing: Box<OutgoingConnection<P>>,
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

    /// Announcement from the linear chain.
    ///
    /// Used to track validator sets.
    #[from]
    LinearChainAnnouncement(#[serde(skip_serializing)] LinearChainAnnouncement),
    /// The set of active and upcoming validators changed.
    ValidatorsChanged {
        /// Active validators (current and upcoming era).
        active_validators: Box<HashSet<PublicKey>>,
        /// Upcoming validators (for era + 2).
        upcoming_validators: Box<HashSet<PublicKey>>,
    },
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
                span: _,
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
            Event::SweepSymmetries => {
                write!(f, "sweep connection symmetries")
            }
            Event::LinearChainAnnouncement(ann) => {
                write!(f, "linear chain announcement: {}", ann)
            }
            Event::ValidatorsChanged {
                active_validators,
                upcoming_validators,
            } => {
                write!(
                    f,
                    "validators changed (active {}, upcoming {})",
                    active_validators.len(),
                    upcoming_validators.len()
                )
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
        /// The public key the peer is validating with, if any.
        peer_consensus_public_key: Option<PublicKey>,
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
                peer_consensus_public_key,
                stream: _,
            } => {
                write!(
                    f,
                    "connection established from {}/{}; public: {}",
                    peer_addr, peer_id, public_addr,
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
pub enum OutgoingConnection<P> {
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
        peer_consensus_public_key: Option<PublicKey>,
        /// Sink for outgoing messages.
        #[serde(skip_serializing)]
        sink: SplitSink<FramedTransport<P>, Arc<Message<P>>>,
    },
}

impl<P> Display for OutgoingConnection<P> {
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
                sink: _,
            } => {
                write!(f, "connection established to {}/{}", peer_addr, peer_id)?;

                if let Some(public_key) = peer_consensus_public_key {
                    write!(f, " [{}]", public_key)
                } else {
                    f.write_str(" [no validator id]")
                }
            }
        }
    }
}
