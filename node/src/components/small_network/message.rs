use std::fmt::{self, Debug, Display, Formatter};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Message<P> {
    Handshake { network_name: String },
    Payload(P),
}

impl<P: Display> Display for Message<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::Handshake { network_name } => write!(f, "handshake: {}", network_name),
            Message::Payload(payload) => write!(f, "payload: {}", payload),
        }
    }
}
