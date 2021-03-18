use std::{
    fmt::{self, Debug, Display, Formatter},
    net::SocketAddr,
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Message<P> {
    Handshake {
        /// Network we are connected to.
        network_name: String,
        /// The public address of the node connecting.
        public_address: SocketAddr,
    },
    Payload(P),
}

impl<P: Display> Display for Message<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::Handshake {
                network_name,
                public_address,
            } => write!(
                f,
                "handshake: {}, public addr: {}",
                network_name, public_address
            ),
            Message::Payload(payload) => write!(f, "payload: {}", payload),
        }
    }
}
