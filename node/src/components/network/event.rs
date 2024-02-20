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
    error::{ConnectionError, MessageReceiverError, MessageSenderError},
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
const_assert!(_NETWORK_EVENT_SIZE < 65);

/// A network event.
#[derive(Debug, From, Serialize)]
pub(crate) enum Event<P>
where
    // Note: See notes on the `OutgoingConnection`'s `P: Serialize` trait bound for details.
    P: Serialize,
{
    Initialize,

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
            Event::IncomingMessage {
                peer_id: node_id,
                msg,
                span: _,
                ticket: _,
            } => write!(f, "msg from {}: {}", node_id, msg),
            Event::NetworkRequest { req } => write!(f, "request: {}", req),
            Event::NetworkInfoRequest { req } => write!(f, "request: {}", req),
            Event::GossipOurAddress => write!(f, "gossip our address"),
            Event::PeerAddressReceived(gossiped_address) => {
                write!(f, "received gossiped peer address {}", gossiped_address)
            }
            Event::BlocklistAnnouncement(ann) => {
                write!(f, "handling blocklist announcement: {}", ann)
            }
        }
    }
}
