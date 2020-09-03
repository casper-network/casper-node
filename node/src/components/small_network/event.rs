use std::{
    fmt::{self, Debug, Display, Formatter},
    io,
    net::SocketAddr,
};

use derive_more::From;
use tokio::net::TcpStream;

use super::{Error, Message, NodeId, Transport};
use crate::effect::requests::NetworkRequest;

#[derive(Debug, From)]
pub enum Event<P> {
    /// Connection to the known node failed.
    BootstrappingFailed { error: Error },
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
    IncomingMessage { node_id: NodeId, msg: Message<P> },
    /// Incoming connection closed.
    IncomingClosed {
        result: io::Result<()>,
        node_id: NodeId,
        address: SocketAddr,
    },

    /// A new outgoing connection was successfully established.
    OutgoingEstablished {
        node_id: NodeId,
        transport: Transport,
    },
    /// An outgoing connection failed to connect or was terminated.
    OutgoingFailed {
        node_id: NodeId,
        error: Option<Error>,
    },

    /// Incoming network request.
    #[from]
    NetworkRequest { req: NetworkRequest<NodeId, P> },
}

impl<P: Display> Display for Event<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::BootstrappingFailed { error } => write!(f, "root failed: {}", error),
            Event::IncomingNew { address, .. } => write!(f, "incoming connection from {}", address),
            Event::IncomingHandshakeCompleted { result, address } => {
                write!(f, "handshake from {}, is_err {}", address, result.is_err())
            }
            Event::IncomingMessage { node_id, msg } => write!(f, "msg from {}: {}", node_id, msg),
            Event::IncomingClosed { address, .. } => {
                write!(f, "closed connection from {}", address)
            }
            Event::OutgoingEstablished { node_id, .. } => {
                write!(f, "established outgoing to {}", node_id)
            }
            Event::OutgoingFailed { node_id, error } => write!(
                f,
                "failed outgoing {}: (is_err {})",
                node_id,
                error.is_some()
            ),
            Event::NetworkRequest { req } => write!(f, "request: {}", req),
        }
    }
}
