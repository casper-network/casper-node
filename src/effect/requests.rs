use std::fmt;

use super::Responder;

#[derive(Debug)]
pub(crate) enum NetworkRequest<I, P> {
    /// Send a message on the network to a specific peer.
    SendMessage {
        dest: I,
        payload: P,
        responder: Responder<()>,
    },
    /// Send a message on the network to all peers.
    BroadcastMessage {
        payload: P,
        responder: Responder<()>,
    },
}

impl<I, P> fmt::Display for NetworkRequest<I, P>
where
    I: fmt::Display,
    P: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkRequest::SendMessage { dest, payload, .. } => {
                write!(f, "send to {}: {}", dest, payload)
            }
            NetworkRequest::BroadcastMessage { payload, .. } => write!(f, "broadcast: {}", payload),
        }
    }
}
