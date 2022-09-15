use std::{
    fmt::{self, Debug, Display, Formatter},
    io, mem,
    net::SocketAddr,
    sync::Arc,
};

use casper_types::PublicKey;
use derive_more::From;
use futures::stream::{SplitSink, SplitStream};
use serde::Serialize;
use tracing::Span;

use super::{error::ConnectionError, FullTransport, GossipedAddress, Message, NodeId};
use crate::{
    effect::{
        announcements::{
            BlocklistAnnouncement, ChainSynchronizerAnnouncement, ContractRuntimeAnnouncement,
        },
        requests::{NetworkInfoRequest, NetworkRequest},
    },
    protocol::Message as ProtocolMessage,
};

const _SMALL_NETWORK_EVENT_SIZE: usize = mem::size_of::<Event<ProtocolMessage>>();
// const_assert!(_SMALL_NETWORK_EVENT_SIZE < 90);

#[test]
fn fix_it() {
    todo!("fix the const assert above")
}

/// A small network event.
#[derive(Debug, From, Serialize)]
pub(crate) enum Event<P> {
    Initialize,

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
    BlocklistAnnouncement(BlocklistAnnouncement),

    /// Contract runtime announcement.
    #[from]
    ContractRuntimeAnnouncement(ContractRuntimeAnnouncement),

    /// Chain synchronizer announcement.
    #[from]
    ChainSynchronizerAnnouncement(ChainSynchronizerAnnouncement),
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

impl<P: Display> Display for Event<P> {
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
            Event::ContractRuntimeAnnouncement(ann) => {
                write!(f, "handling contract runtime announcement: {}", ann)
            }
            Event::SweepOutgoing => {
                write!(f, "sweep outgoing connections")
            }
            Event::ChainSynchronizerAnnouncement(ann) => {
                write!(f, "handling chain synchronizer announcement: {}", ann)
            }
        }
    }
}

/// Outcome of an incoming connection negotiation.
#[derive(Debug, Serialize)]
pub(crate) enum IncomingConnection<P> {
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
        stream: SplitStream<FullTransport<P>>,
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
pub(crate) enum OutgoingConnection<P> {
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
        sink: SplitSink<FullTransport<P>, Arc<Message<P>>>,
        /// Holds the information whether the remote node is syncing.
        is_syncing: bool,
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
                is_syncing,
            } => {
                write!(
                    f,
                    "connection established to {}/{}, is_syncing: {}",
                    peer_addr, peer_id, is_syncing
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
