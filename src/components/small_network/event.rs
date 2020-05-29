use std::{
    fmt::{self, Debug, Display, Formatter},
    io,
    net::SocketAddr,
};

use tokio::net::TcpStream;

use super::{Message, NodeId, Transport};
use crate::tls::TlsCert;

#[derive(Debug)]
pub(crate) enum Event<P> {
    /// Connection to the root node succeeded.
    RootConnected { cert: TlsCert, transport: Transport },
    /// Connection to the root node failed.
    RootFailed { error: anyhow::Error },
    /// A new TCP connection has been established from an incoming connection.
    IncomingNew { stream: TcpStream, addr: SocketAddr },
    /// The TLS handshake completed on the incoming connection.
    IncomingHandshakeCompleted {
        result: anyhow::Result<(NodeId, Transport)>,
        addr: SocketAddr,
    },
    /// Received network message.
    IncomingMessage { node_id: NodeId, msg: Message<P> },
    /// Incoming connection closed.
    IncomingClosed {
        result: io::Result<()>,
        addr: SocketAddr,
    },

    /// A new outgoing connection was successfully established.
    OutgoingEstablished {
        node_id: NodeId,
        transport: Transport,
    },
    /// An outgoing connection failed to connect or was terminated.
    OutgoingFailed {
        node_id: NodeId,
        attempt_count: u32,
        error: Option<anyhow::Error>,
    },
}

impl<P: Display> Display for Event<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::RootConnected { cert, .. } => {
                write!(f, "root connected @ {}", cert.public_key_fingerprint())
            }
            Event::RootFailed { error } => write!(f, "root failed: {}", error),
            Event::IncomingNew { addr, .. } => write!(f, "incoming connection from {}", addr),
            Event::IncomingHandshakeCompleted { result, addr } => {
                write!(f, "handshake from {}, is_err {}", addr, result.is_err())
            }
            Event::IncomingMessage { node_id, msg } => write!(f, "msg from {}: {}", node_id, msg),
            Event::IncomingClosed { addr, .. } => write!(f, "closed connection from {}", addr),
            Event::OutgoingEstablished { node_id, .. } => {
                write!(f, "established outgoing to {}", node_id)
            }
            Event::OutgoingFailed {
                node_id,
                attempt_count,
                error,
            } => write!(
                f,
                "failed outgoing {} [{}]: (is_err {})",
                node_id,
                attempt_count,
                error.is_some()
            ),
        }
    }
}
